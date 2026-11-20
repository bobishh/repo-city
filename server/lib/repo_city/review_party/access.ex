defmodule RepoCity.ReviewParty.Access do
  alias RepoCity.GitHub.Client
  alias RepoCity.ReviewParty.Protocol

  def account(nil), do: {:error, 401, "Sign in with GitHub."}
  def account("rcparty_" <> _), do: account(nil)

  def account(credential) do
    with {:ok, user} <- get("/user", credential), do: actor(user)
  end

  def invitee(login, credential) do
    if Protocol.login?(login) do
      with {:ok, user} <- get("/users/" <> login, credential), do: actor(user)
    else
      {:error, 400, "Invalid GitHub login."}
    end
  end

  def pin(comparison, credential) do
    root = repo_path(comparison.owner, comparison.repo)

    with {:ok, base_repo} <- get(root, credential),
         {:ok, id} <- repository_id(base_repo),
         {:ok, comparison} <- pin_fork(comparison, credential, root),
         :ok <- verify_commits(comparison, root, credential) do
      {:ok, comparison, id}
    end
  end

  def verify(comparison, expected_id, credential) do
    root = repo_path(comparison.owner, comparison.repo)

    with {:ok, base_repo} <- get(root, credential),
         {:ok, ^expected_id} <- repository_id(base_repo),
         :ok <- verify_commits(comparison, root, credential),
         :ok <- verify_pull(comparison, root, credential) do
      :ok
    else
      {:ok, _wrong_repo} -> unavailable()
      error -> error
    end
  end

  defp pin_fork(%{pr_number: number} = comparison, credential, root) do
    with {:ok, %{"head" => %{"repo" => %{"full_name" => name}}}} when is_binary(name) <-
           get(root <> "/pulls/#{number}", credential),
         [owner, repo] <- String.split(name, "/"),
         true <- Protocol.name?(owner) && Protocol.name?(repo) do
      {:ok, Map.merge(comparison, %{head_owner: owner, head_repo: repo})}
    else
      {:error, _, _} = error -> error
      _ -> {:error, 409, "Pull request source repository is unavailable."}
    end
  end

  defp pin_fork(comparison, _credential, _root), do: {:ok, comparison}

  defp verify_pull(%{pr_number: number}, root, credential) do
    with {:ok, _pull} <- get(root <> "/pulls/#{number}", credential), do: :ok
  end

  defp verify_pull(_comparison, _root, _credential), do: :ok

  defp verify_commits(comparison, root, credential) do
    %{head_sha: head_sha, base_sha: base_sha} = comparison

    head_root =
      case comparison do
        %{head_owner: owner, head_repo: repo} -> repo_path(owner, repo)
        _ -> root
      end

    with {:ok, %{"sha" => ^head_sha}} <- get(head_root <> "/commits/" <> head_sha, credential),
         {:ok, %{"sha" => ^base_sha}} <- get(root <> "/commits/" <> base_sha, credential) do
      :ok
    else
      {:error, _, _} = error -> error
      _ -> unavailable()
    end
  end

  defp repo_path(owner, repo), do: "/repos/#{owner}/#{repo}"

  defp repository_id(%{"id" => id}) when is_integer(id) and id > 0,
    do: {:ok, Integer.to_string(id)}

  defp repository_id(_), do: unavailable()

  defp actor(%{"id" => id, "login" => login} = user) when is_integer(id) and id > 0 do
    id = Integer.to_string(id)

    if Protocol.identity?(id) && Protocol.login?(login) do
      avatar =
        case user["avatar_url"] do
          "https://avatars.githubusercontent.com/" <> _ = url when byte_size(url) <= 512 -> url
          _ -> ""
        end

      {:ok, %{id: id, login: login, avatar_url: avatar}}
    else
      invalid_account()
    end
  end

  defp actor(_), do: invalid_account()

  defp get(path, credential) do
    case Client.api(path, credential) do
      {:ok, %{status: 200, body: body}} ->
        case Jason.decode(body) do
          {:ok, value} when is_map(value) -> {:ok, value}
          _ -> upstream_error(502)
        end

      {:ok, %{status: 401}} ->
        upstream_error(401)

      {:ok, %{status: status}} when status in [403, 404] ->
        upstream_error(403)

      _ ->
        upstream_error(502)
    end
  end

  defp upstream_error(status),
    do: {:error, status, "GitHub account or repository access could not be verified."}

  defp invalid_account, do: {:error, 502, "Invalid GitHub account."}
  defp unavailable, do: {:error, 409, "Pinned comparison is unavailable."}
end
