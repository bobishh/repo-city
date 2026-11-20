defmodule RepoCity.GitHub.Cookie do
  import Plug.Conn

  @session "__Host-repo-city"
  @flow "__Host-repo-city-oauth"

  def session(conn, secret), do: read(conn, @session, secret)
  def flow(conn, secret), do: read(conn, @flow, secret)

  def put_session(conn, secret, payload, seconds),
    do: put(conn, @session, secret, payload, seconds)

  def put_flow(conn, secret, payload), do: put(conn, @flow, secret, payload, 600)
  def clear_session(conn), do: set(conn, @session, "", 0)
  def clear_flow(conn), do: set(conn, @flow, "", 0)

  defp read(conn, name, secret) do
    now = System.os_time(:millisecond)

    with value when is_binary(value) and byte_size(value) <= 3600 <-
           fetch_cookies(conn).req_cookies[name],
         [iv, encrypted] <- String.split(value, ".", parts: 2),
         {:ok, <<_::96>> = iv} <- Base.url_decode64(iv, padding: false),
         {:ok, raw} when byte_size(raw) > 16 <- Base.url_decode64(encrypted, padding: false),
         cipher_size = byte_size(raw) - 16,
         <<cipher::binary-size(^cipher_size), tag::binary-size(16)>> = raw,
         data when is_binary(data) <-
           :crypto.crypto_one_time_aead(:aes_256_gcm, key(secret), iv, cipher, name, tag, false),
         {:ok, %{"expires" => expires} = payload} when is_integer(expires) and expires > now <-
           Jason.decode(data) do
      payload
    else
      _ -> nil
    end
  end

  defp put(conn, name, secret, payload, seconds) do
    iv = :crypto.strong_rand_bytes(12)

    data =
      payload
      |> Map.put("expires", System.os_time(:millisecond) + seconds * 1000)
      |> Jason.encode!()

    {cipher, tag} =
      :crypto.crypto_one_time_aead(:aes_256_gcm, key(secret), iv, data, name, 16, true)

    set(conn, name, encode(iv) <> "." <> encode(cipher <> tag), seconds)
  end

  defp set(conn, name, value, seconds) do
    put_resp_cookie(conn, name, value,
      secure: true,
      http_only: true,
      same_site: "Lax",
      path: "/",
      max_age: seconds
    )
  end

  defp encode(value), do: Base.url_encode64(value, padding: false)
  defp key(secret), do: :crypto.hash(:sha256, "repo-city:github:v1:" <> secret)
end
