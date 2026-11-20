import Config

config :repo_city, RepoCityWeb.Endpoint,
  adapter: Bandit.PhoenixAdapter,
  server: false

config :repo_city, :public_origin, "https://repo-city.meta-uber-engineer.dev"
config :phoenix, :json_library, Jason

if config_env() == :test do
  config :repo_city,
    github_request: [plug: {Req.Test, :github}],
    github_client_id: "test-client",
    github_client_secret: "test-secret",
    github_app_slug: "repo-city"
end
