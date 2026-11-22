import Config

if config_env() != :test do
  config :repo_city,
    public_origin: System.get_env("PUBLIC_ORIGIN", "https://repo-city.meta-uber-engineer.dev"),
    github_client_id: System.get_env("GITHUB_CLIENT_ID"),
    github_client_secret: System.get_env("GITHUB_CLIENT_SECRET"),
    github_app_slug: System.get_env("GITHUB_APP_SLUG"),
    review_parties_enabled: System.get_env("REVIEW_PARTIES_ENABLED") == "true",
    review_party_ice_servers: System.get_env("REVIEW_PARTY_ICE_SERVERS"),
    review_party_turn_secret: System.get_env("REVIEW_PARTY_TURN_SECRET"),
    review_party_turn_urls: System.get_env("REVIEW_PARTY_TURN_URLS", ""),
    static_root: Path.expand(System.get_env("STATIC_ROOT", "../web/dist")),
    browser_script: System.get_env("BROWSER_SCRIPT"),
    revision: System.get_env("REPO_CITY_REVISION")

  for {setting, variable, default} <- [
        {:party_active_rooms, "REVIEW_PARTY_ACTIVE_ROOMS", 256},
        {:party_room_bytes, "REVIEW_PARTY_ROOM_BYTES", 131_072},
        {:party_messages_per_second, "REVIEW_PARTY_MESSAGES_PER_SECOND", 120},
        {:party_pending_invites, "REVIEW_PARTY_PENDING_INVITES", 256}
      ] do
    value = System.get_env(variable, Integer.to_string(default))

    case Integer.parse(value) do
      {limit, ""} when limit > 0 -> config :repo_city, setting, limit
      _ -> raise "#{variable} must be a positive integer"
    end
  end

  address = System.get_env("BIND_ADDRESS", "0.0.0.0")
  {:ok, ip} = :inet.parse_address(String.to_charlist(address))

  config :repo_city, RepoCityWeb.Endpoint,
    server: System.get_env("PHX_SERVER") == "true",
    http: [
      ip: ip,
      port: String.to_integer(System.get_env("PORT", "8080"))
    ]
end
