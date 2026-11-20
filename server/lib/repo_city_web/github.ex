defmodule RepoCityWeb.GitHub do
  import Plug.Conn
  import RepoCityWeb.Router, only: [json: 3]
  alias RepoCity.GitHub.{Client, Cookie, PublicCache}

  @public_path ~r"\A/repos/([\w.-]{1,100})/([\w.-]{1,100})/git/(blobs|trees)/[a-f\d]{40,64}\z"i

  def call(%{path_info: ["github", "public" | _]} = conn) do
    conn =
      conn
      |> put_resp_header("cache-control", "no-store")
      |> put_resp_header("x-content-type-options", "nosniff")

    with "GET" <- conn.method,
         "/github/public" <> path <- conn.request_path,
         [_, owner, repo, kind] <- Regex.run(@public_path, path),
         true <- owner not in [".", ".."] and repo not in [".", ".."],
         true <-
           conn.query_string == "" ||
             (String.downcase(kind) == "trees" && conn.query_string == "recursive=1"),
         target = String.downcase(path) <> query_suffix(conn.query_string),
         {:ok, cache, body} <- PublicCache.read(target) do
      conn
      |> put_resp_header("x-repo-city-cache", cache)
      |> put_resp_content_type("application/json")
      |> send_resp(200, body)
    else
      {:error, status} -> json(conn, status, %{error: "Public Git object unavailable."})
      _ when conn.method != "GET" -> json(conn, 405, %{error: "Public Git object unavailable."})
      _ -> json(conn, 404, %{error: "Public Git object unavailable."})
    end
  end

  def call(conn) do
    conn =
      conn
      |> fetch_query_params()
      |> put_resp_header("cache-control", "private, no-store")
      |> put_resp_header("referrer-policy", "no-referrer")

    id = Application.get_env(:repo_city, :github_client_id)
    secret = Application.get_env(:repo_city, :github_client_secret)
    enabled = nonempty?(id) && nonempty?(secret)

    case {conn.method, conn.path_info, enabled} do
      {"GET", ["github", "session"], _} ->
        session = if enabled, do: Cookie.session(conn, secret)
        slug = Application.get_env(:repo_city, :github_app_slug)

        install_url =
          if matches?(slug, ~r/\A[a-z\d-]+\z/),
            do: "https://github.com/apps/#{slug}/installations/new"

        json(conn, 200, %{
          enabled: enabled,
          login: session && session["login"],
          installUrl: install_url
        })

      {_, _, false} ->
        json(conn, 503, %{error: "GitHub sign-in is unavailable."})

      {"GET", ["github", "login"], true} ->
        login(conn, id, secret)

      {"GET", ["github", "callback"], true} ->
        callback(conn, id, secret)

      {"POST", ["github", "logout"], true} ->
        if same_origin?(conn),
          do: conn |> Cookie.clear_session() |> send_resp(204, ""),
          else: json(conn, 403, %{error: "Invalid origin."})

      _ ->
        RepoCityWeb.GitHubProxy.call(conn, Cookie.session(conn, secret))
    end
  end

  def same_origin?(conn), do: get_req_header(conn, "origin") == [origin()]
  defp origin, do: Application.fetch_env!(:repo_city, :public_origin)
  defp callback_url, do: origin() <> "/github/callback"

  defp login(conn, id, secret) do
    verifier = random()
    state = random()

    params = %{
      client_id: id,
      redirect_uri: callback_url(),
      state: state,
      code_challenge: encode(:crypto.hash(:sha256, verifier)),
      code_challenge_method: "S256"
    }

    payload = %{
      "verifier" => verifier,
      "state" => state,
      "returnTo" => return_path(conn.query_params["return"])
    }

    conn
    |> Cookie.put_flow(secret, payload)
    |> redirect("https://github.com/login/oauth/authorize?" <> URI.encode_query(params))
  end

  defp callback(conn, id, secret) do
    flow = Cookie.flow(conn, secret)
    conn = Cookie.clear_flow(conn)
    code = conn.query_params["code"]
    state = conn.query_params["state"]
    return = return_path(flow && flow["returnTo"])

    with %{"verifier" => verifier, "state" => expected} <- flow,
         true <-
           nonempty?(code) && nonempty?(state) && Plug.Crypto.secure_compare(state, expected),
         {:ok, %{status: status, body: body}} when status in 200..299 <-
           Client.exchange(
             client_id: id,
             client_secret: secret,
             code: code,
             redirect_uri: callback_url(),
             code_verifier: verifier
           ),
         {:ok, %{"access_token" => "ghu_" <> _ = token} = exchange} <- Jason.decode(body),
         {:ok, %{status: status, body: body}} when status in 200..299 <-
           Client.api("/user", token),
         {:ok, %{"login" => login}} <- Jason.decode(body),
         true <- matches?(login, ~r/\A[\w-]+\z/) do
      age =
        case exchange["expires_in"] do
          expires when is_integer(expires) and expires != 0 -> expires |> min(28_800) |> max(1)
          _ -> 28_800
        end

      conn
      |> Cookie.put_session(secret, %{"token" => token, "login" => login}, age)
      |> redirect(oauth_result(return, "connected"))
    else
      _ -> redirect(conn, oauth_result(return, "failed"))
    end
  end

  defp return_path(value) when is_binary(value) and byte_size(value) <= 1200 do
    case URI.new(value) do
      {:ok, %URI{scheme: nil, host: nil, path: "/", query: query}} ->
        "/" <> query_suffix(query || "")

      _ ->
        "/"
    end
  end

  defp return_path(_), do: "/"

  defp oauth_result(path, value) do
    params =
      URI.parse(path).query |> Kernel.||("") |> URI.decode_query() |> Map.put("github", value)

    "/?" <> URI.encode_query(params)
  end

  defp redirect(conn, url), do: conn |> put_resp_header("location", url) |> send_resp(303, "")
  defp query_suffix(""), do: ""
  defp query_suffix(query), do: "?" <> query
  defp random, do: encode(:crypto.strong_rand_bytes(32))
  defp encode(bytes), do: Base.url_encode64(bytes, padding: false)
  defp nonempty?(value), do: is_binary(value) && value != ""
  defp matches?(value, pattern), do: is_binary(value) && Regex.match?(pattern, value)
end
