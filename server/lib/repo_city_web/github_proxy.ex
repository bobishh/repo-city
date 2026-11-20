defmodule RepoCityWeb.GitHubProxy do
  import Plug.Conn
  import RepoCityWeb.Router, only: [json: 3]
  alias RepoCity.GitHub.{Client, Cookie, ReviewComment}
  alias RepoCityWeb.GitHub

  @repo ~r"\A/repos/[\w.-]+/[\w.-]+(?:/(?:branches|tags|commits|commits/[\w.%~-]+|compare/[\w.%~:-]+|pulls(?:/[1-9]\d{0,14}(?:/comments)?)?|git/(?:trees|blobs)/[a-f\d]{40,64}))?\z"i
  @account ~r"\A/(?:user/(?:starred|installations(?:/\d+/repositories)?)|users/[\w-]+/repos)\z"
  @review ~r"\A/github/review/([\w.-]{1,100})/([\w.-]{1,100})/([1-9]\d{0,14})(?:/comments/([1-9]\d{0,15})/eyes)?\z"

  @blame """
  query CityBlame($owner: String!, $name: String!, $oid: GitObjectID!, $path: String!) {
    repository(owner: $owner, name: $name) {
      object(oid: $oid) {
        ... on Commit {
          blame(path: $path) {
            ranges { startingLine endingLine commit { abbreviatedOid committedDate author { name user { login } } } }
          }
        }
      }
    }
  }
  """

  @threads """
  query CityReviewThreads($owner: String!, $repo: String!, $number: Int!, $cursor: String) {
    repository(owner: $owner, name: $repo) {
      pullRequest(number: $number) {
        headRefOid
        reviewThreads(first: 50, after: $cursor) {
          pageInfo { endCursor hasNextPage }
          nodes { isOutdated isResolved path line startLine originalLine originalStartLine diffSide startDiffSide
            comments(first: 50) { nodes { id reactionGroups { content viewerHasReacted } } }
          }
        }
      }
    }
  }
  """

  def call(%{method: "GET", request_path: "/github/blame"} = conn, session) do
    authorized(conn, session, "Sign in with GitHub.", fn session -> blame(conn, session) end)
  end

  def call(%{method: "GET", request_path: "/github/api" <> path} = conn, session) do
    case rest_target(path, conn.query_params) do
      nil ->
        unknown(conn)

      target ->
        authorized(conn, session, "Sign in with GitHub.", fn %{"token" => token} ->
          rest(conn, target, token)
        end)
    end
  end

  def call(conn, session) do
    case Regex.run(@review, conn.request_path) do
      [_, owner, repo, number] when conn.method == "GET" ->
        authorized(conn, session, "Sign in with GitHub.", fn session ->
          threads(conn, session, owner, repo, number)
        end)

      [_, owner, repo, number] when conn.method == "POST" ->
        write(conn, session, "Sign in with GitHub to publish.", fn session ->
          comment(conn, session, "/repos/#{owner}/#{repo}/pulls/#{number}")
        end)

      [_, owner, repo, number, comment] when conn.method == "POST" ->
        write(conn, session, "Sign in with GitHub to mark a comment viewed.", fn session ->
          reaction(conn, session, "/repos/#{owner}/#{repo}/pulls", number, comment)
        end)

      _ when conn.method != "GET" ->
        json(conn, 405, %{error: "Method not allowed."})

      _ ->
        unknown(conn)
    end
  end

  defp rest_target("/search/commits", %{"q" => q}) when is_binary(q) and byte_size(q) <= 880 do
    if String.length(q) <= 220 &&
         Regex.match?(~r"\Arepo:[\w.-]+/[\w.-]+(?: \"[\p{L}\p{N}_-]+\"){1,8}\z"u, q) do
      "/search/commits?" <> URI.encode_query(q: q, per_page: 8)
    end
  end

  defp rest_target(path, params) do
    if (Regex.match?(@repo, path) || Regex.match?(@account, path)) &&
         valid_path?(URI.decode(path)) do
      with {:ok, query} <- rest_query(path, params) do
        if query == "", do: path, else: path <> "?" <> query
      end
    end
  end

  defp valid_path?(path) do
    !String.contains?(path, ["\\", <<0>>]) &&
      !Enum.any?(String.split(path, "/"), &(&1 in [".", ".."]))
  end

  defp rest_query(path, params) do
    keys = ["per_page", "page", "recursive", "sort"]
    keys = if String.ends_with?(path, "/pulls"), do: ["state" | keys], else: keys
    query = Map.take(params, keys)

    if Enum.all?(query, fn
         {"state", value} -> value in ["open", "closed", "all"]
         {"sort", value} -> value == "pushed"
         {_key, value} -> is_binary(value) && Regex.match?(~r/\A\d{1,3}\z/, value)
       end) do
      {:ok, URI.encode_query(query)}
    end
  end

  defp rest(conn, path, token) do
    case Client.api(path, token) do
      {:ok, %{status: status} = response} when status in 300..399 ->
        with location when is_binary(location) <- header(response, "location"),
             {:ok, %URI{} = uri} <- URI.new(location),
             %URI{scheme: "https", host: "api.github.com", port: 443, userinfo: nil, path: next} =
               target <-
               URI.merge("https://api.github.com", uri),
             true <-
               is_binary(next) && Regex.match?(~r"\A/repositories/\d+(?:/|\z)", next) &&
                 valid_path?(URI.decode(next)) do
          renamed = next <> if(target.query, do: "?" <> target.query, else: "")

          case Client.api(renamed, token) do
            {:ok, %{status: status}} when status in 300..399 -> redirected(conn)
            result -> relay(conn, result)
          end
        else
          _ -> redirected(conn)
        end

      result ->
        relay(conn, result)
    end
  end

  defp blame(conn, %{"token" => token}) do
    %{"owner" => owner, "repo" => repo, "sha" => oid, "path" => path} =
      Map.merge(%{"owner" => nil, "repo" => nil, "sha" => nil, "path" => nil}, conn.query_params)

    if Enum.all?([owner, repo], &repo_name?/1) &&
         is_binary(oid) && Regex.match?(~r/\A[a-f\d]{40}\z/i, oid) &&
         ReviewComment.valid_file?(path) do
      case graphql(@blame, %{owner: owner, name: repo, oid: oid, path: path}, token) do
        {:ok, %{status: 401}} = result ->
          relay(conn, result)

        {:ok, %{status: 403}} ->
          json(conn, 403, %{error: "Line history unavailable."})

        {:ok, %{status: status, body: body}} when status in 200..299 ->
          with {:ok, value} <- Jason.decode(body),
               ranges when is_list(ranges) <-
                 get_in(value, ["data", "repository", "object", "blame", "ranges"]) do
            json(conn, 200, %{ranges: ranges})
          else
            _ -> json(conn, 404, %{error: "Line history unavailable."})
          end

        _ ->
          json(conn, 502, %{error: "Line history unavailable."})
      end
    else
      json(conn, 400, %{error: "Invalid blame target."})
    end
  end

  defp threads(conn, %{"token" => token}, owner, repo, number) do
    number = String.to_integer(number)
    cursor = conn.query_params["cursor"]

    if number <= 2_147_483_647 &&
         (cursor in [nil, ""] ||
            (is_binary(cursor) && Regex.match?(~r"\A[\w+/=-]{1,300}\z", cursor))) do
      case graphql(@threads, %{owner: owner, repo: repo, number: number, cursor: cursor}, token) do
        {:ok, %{status: status, body: body}} when status in 200..299 ->
          with {:ok, value} <- Jason.decode(body),
               %{"reviewThreads" => threads} = pull when is_map(threads) <-
                 get_in(value, ["data", "repository", "pullRequest"]) do
            json(conn, 200, pull)
          else
            _ -> json(conn, 404, %{error: "Thread status unavailable."})
          end

        result ->
          relay(conn, result)
      end
    else
      json(conn, 400, %{error: "Invalid review request."})
    end
  end

  defp comment(conn, %{"token" => token, "login" => login}, target) do
    {conn, input} = read_comment(conn)

    {status, body} =
      ReviewComment.publish(input, login, fn suffix, options ->
        Client.api(target <> suffix, token, options)
      end)

    conn = if status == 401, do: Cookie.clear_session(conn), else: conn
    json(conn, status, body)
  end

  defp read_comment(conn) do
    with [content_type | _] <- get_req_header(conn, "content-type"),
         true <- String.starts_with?(String.downcase(content_type), "application/json") do
      case read_body(conn, length: 96 * 1024, read_length: 96 * 1024) do
        {:ok, body, conn} -> {conn, json_or_nil(body)}
        {:more, _body, conn} -> {conn, nil}
        {:error, _} -> {conn, nil}
      end
    else
      _ -> {conn, nil}
    end
  end

  defp reaction(conn, %{"token" => token, "login" => login}, base, number, id) do
    target = base <> "/comments/" <> id

    case Client.api(target, token) do
      {:ok, %{status: status, body: body}} when status in 200..299 ->
        expected = "https://api.github.com#{base}/#{number}"

        case json_or_nil(body) do
          %{"pull_request_url" => url} when is_binary(url) ->
            if String.downcase(url) == String.downcase(expected),
              do: mark_viewed(conn, target, token, login),
              else: wrong_pull(conn)

          _ ->
            wrong_pull(conn)
        end

      result ->
        relay(conn, result)
    end
  end

  defp mark_viewed(conn, target, token, login) do
    case Client.api(target <> "/reactions", token, method: :post, json: %{content: "eyes"}) do
      {:ok, %{status: 403}} ->
        json(conn, 403, %{error: "Marking viewed needs Pull requests: write."})

      {:ok, %{status: status, body: body}} when status in 200..299 ->
        with %{"content" => "eyes", "user" => %{"login" => author}} when is_binary(author) <-
               json_or_nil(body),
             true <- String.downcase(author) == String.downcase(login) do
          json(conn, 200, %{viewed: true})
        else
          _ -> json(conn, 502, %{error: "GitHub did not confirm your reaction."})
        end

      result ->
        relay(conn, result)
    end
  end

  defp write(conn, session, message, work) do
    if GitHub.same_origin?(conn),
      do: authorized(conn, session, message, work),
      else: json(conn, 403, %{error: "Invalid origin."})
  end

  defp authorized(conn, %{"token" => token, "login" => login} = session, _message, work)
       when is_binary(token) and is_binary(login) do
    if repo_name_segments?(conn), do: work.(session), else: unknown(conn)
  end

  defp authorized(conn, _session, message, _work), do: json(conn, 401, %{error: message})

  defp repo_name_segments?(%{path_info: ["github", "review", owner, repo | _]}),
    do: repo_name?(owner) && repo_name?(repo)

  defp repo_name_segments?(_conn), do: true

  defp repo_name?(value),
    do: is_binary(value) && value not in [".", ".."] && Regex.match?(~r/\A[\w.-]{1,100}\z/, value)

  defp graphql(query, variables, token),
    do: Client.api("/graphql", token, method: :post, json: %{query: query, variables: variables})

  defp json_or_nil(body) do
    case Jason.decode(body) do
      {:ok, value} -> value
      _ -> nil
    end
  end

  defp relay(conn, {:ok, %{status: status, body: body} = response}) do
    conn = if status == 401, do: Cookie.clear_session(conn), else: conn
    conn = if link = header(response, "link"), do: put_resp_header(conn, "link", link), else: conn

    conn
    |> put_resp_content_type("application/json")
    |> put_resp_header("x-content-type-options", "nosniff")
    |> send_resp(status, body)
  end

  defp relay(conn, {:error, _}), do: json(conn, 502, %{error: "Could not reach GitHub."})
  defp header(response, key), do: response |> Req.Response.get_header(key) |> List.first()
  defp unknown(conn), do: json(conn, 404, %{error: "Unknown GitHub route."})
  defp redirected(conn), do: json(conn, 404, %{error: "GitHub redirected this request."})

  defp wrong_pull(conn),
    do: json(conn, 404, %{error: "This comment is outside the selected pull request."})
end
