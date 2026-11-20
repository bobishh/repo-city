import Config

if config_env() != :test do
  config :repo_city,
    public_origin: System.get_env("PUBLIC_ORIGIN", "https://repo-city.meta-uber-engineer.dev"),
    github_client_id: System.get_env("GITHUB_CLIENT_ID"),
    github_client_secret: System.get_env("GITHUB_CLIENT_SECRET"),
    github_app_slug: System.get_env("GITHUB_APP_SLUG"),
    static_root: Path.expand(System.get_env("STATIC_ROOT", "../web/dist")),
    revision: System.get_env("REPO_CITY_REVISION")

  address = System.get_env("BIND_ADDRESS", "0.0.0.0")
  {:ok, ip} = :inet.parse_address(String.to_charlist(address))

  config :repo_city, RepoCityWeb.Endpoint,
    server: System.get_env("PHX_SERVER") == "true",
    http: [
      ip: ip,
      port: String.to_integer(System.get_env("PORT", "8080"))
    ]
end
