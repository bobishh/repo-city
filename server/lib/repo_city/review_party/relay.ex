defmodule RepoCity.ReviewParty.Relay do
  @credential_seconds 3600

  def ice_servers(room_id, account_id) do
    leases(room_id, account_id) ++ stored_servers()
  end

  defp leases(room_id, account_id) do
    secret = Application.get_env(:repo_city, :review_party_turn_secret)
    config = Application.get_env(:repo_city, :review_party_turn_urls, "")

    with secret when is_binary(secret) and byte_size(secret) >= 64 <- secret,
         urls = config |> String.split(",") |> Enum.map(&String.trim/1),
         true <- length(urls) in 1..8 && Enum.all?(urls, &url?(&1, ~r/\Aturns?:[^\s]+\z/)) do
      expires = System.os_time(:second) + @credential_seconds

      identity =
        sign(:sha256, secret, room_id <> ":" <> account_id) |> Base.url_encode64(padding: false)

      username = "#{expires}:#{identity}"
      credential = sign(:sha, secret, username) |> Base.encode64()
      [%{urls: urls, username: username, credential: credential}]
    else
      _ -> []
    end
  end

  defp stored_servers do
    with config when is_binary(config) <-
           Application.get_env(:repo_city, :review_party_ice_servers),
         {:ok, servers} when is_list(servers) and length(servers) <= 8 <- Jason.decode(config) do
      Enum.flat_map(servers, &valid_server/1)
    else
      _ -> []
    end
  end

  defp valid_server(%{"urls" => urls} = server) do
    urls = List.wrap(urls)
    credentials = Map.take(server, ["username", "credential"])

    if length(urls) in 1..8 &&
         Enum.all?(urls, &url?(&1, ~r/\A(?:stun|stuns|turn|turns):[^\s]+\z/)) &&
         Enum.all?(credentials, fn {_, value} -> is_binary(value) && byte_size(value) <= 512 end) do
      [Map.put(credentials, "urls", urls)]
    else
      []
    end
  end

  defp valid_server(_), do: []

  defp url?(value, regex),
    do: is_binary(value) && byte_size(value) <= 512 && Regex.match?(regex, value)

  defp sign(algorithm, secret, value), do: :crypto.mac(:hmac, algorithm, secret, value)
end
