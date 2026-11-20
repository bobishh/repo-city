defmodule RepoCity.GitHub.Client do
  @api "https://api.github.com"
  @headers [
    {"accept", "application/vnd.github+json"},
    {"user-agent", "Repo City"},
    {"x-github-api-version", "2022-11-28"}
  ]

  def api(path, token, options \\ []) do
    options
    |> Keyword.put(:headers, [{"authorization", "Bearer " <> token} | @headers])
    |> request(@api <> path)
  end

  def public_object(path, etag, into) do
    headers = [{"cache-control", "no-cache"}, {"accept-encoding", "identity"} | @headers]
    headers = if etag, do: [{"if-none-match", etag} | headers], else: headers

    [headers: headers, into: into, compressed: false, receive_timeout: 8_000]
    |> request(@api <> path)
  end

  def exchange(form) do
    [method: :post, form: form, headers: [{"accept", "application/json"}]]
    |> request("https://github.com/login/oauth/access_token")
  end

  defp request(options, url) do
    [
      url: url,
      redirect: false,
      retry: false,
      decode_body: false,
      receive_timeout: 12_000,
      finch: [pool_timeout: 3_000, conn_opts: [transport_opts: [timeout: 3_000]]]
    ]
    |> Keyword.merge(options)
    |> Keyword.merge(Application.get_env(:repo_city, :github_request, []))
    |> Req.request()
  end
end
