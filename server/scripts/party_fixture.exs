if Mix.env() != :test, do: raise("The party fixture requires MIX_ENV=test")

defmodule RepoCity.Test.PartyGitHub do
  import Plug.Conn

  def init(options), do: options

  def call(conn, _options) do
    credential = get_req_header(conn, "authorization")

    case {conn.path_info, credential} do
      {["user"], ["Bearer ghu_party_host"]} -> Req.Test.json(conn, %{id: 1, login: "host"})
      {["user"], ["Bearer ghu_party_guest"]} -> Req.Test.json(conn, %{id: 2, login: "guest"})
      {["users", "guest"], _} -> Req.Test.json(conn, %{id: 2, login: "guest"})
      {["repos", "owner", "demo"], [_]} -> Req.Test.json(conn, %{id: 99, private: true})
      {["repos", "owner", "demo", "commits", sha], [_]} -> Req.Test.json(conn, %{sha: sha})
      _ -> send_resp(conn, 404, "")
    end
  end
end

defmodule RepoCity.Test.PartyFixture do
  import Plug.Conn

  @fixture Path.expand("../../web/tests/fixtures/party-screen.html", __DIR__)
  @modules Path.expand("../../web", __DIR__)

  def init(options), do: options

  def call(%{path_info: ["party-screen"]} = conn, _) do
    conn |> put_resp_content_type("text/html") |> send_file(200, @fixture)
  end

  def call(%{path_info: [module]} = conn, _)
      when module in ["review-party-client.mjs", "review-party-ui.mjs"] do
    conn
    |> put_resp_content_type("text/javascript")
    |> send_file(200, Path.join(@modules, module))
  end

  def call(conn, opts), do: RepoCityWeb.Endpoint.call(conn, opts)
end

port = System.fetch_env!("PORT") |> String.to_integer()
Application.put_env(:repo_city, :public_origin, "http://127.0.0.1:#{port}")
Application.put_env(:repo_city, :review_parties_enabled, true)

Application.put_env(
  :repo_city,
  :review_party_turn_secret,
  System.get_env("REVIEW_PARTY_TURN_SECRET")
)

Application.put_env(
  :repo_city,
  :review_party_turn_urls,
  System.get_env("REVIEW_PARTY_TURN_URLS", "")
)

Application.put_env(
  :repo_city,
  :static_root,
  Path.expand(System.get_env("STATIC_ROOT", "../web/dist"))
)

Application.put_env(:repo_city, :github_request, plug: RepoCity.Test.PartyGitHub)

{:ok, _pid} = Bandit.start_link(plug: RepoCity.Test.PartyFixture, port: port, ip: {127, 0, 0, 1})
Process.sleep(:infinity)
