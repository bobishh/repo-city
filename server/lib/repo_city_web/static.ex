defmodule RepoCityWeb.Static do
  import Plug.Conn

  def call(conn) do
    root = Application.get_env(:repo_city, :static_root)
    conn = normalize_gzip(conn)
    path_info = if conn.path_info == [], do: ["index.html"], else: conn.path_info

    if root do
      opts =
        Plug.Static.init(
          at: "/",
          from: root,
          gzip: true,
          cache_control_for_etags: "no-cache",
          cache_control_for_vsn_requests: "no-cache",
          content_types: %{"repo_city_bg.wasm" => "application/wasm"},
          headers: [{"x-content-type-options", "nosniff"}]
        )

      case Plug.Static.call(%{conn | path_info: path_info}, opts) do
        %{halted: true} = sent -> sent
        not_found -> send_resp(not_found, 404, "")
      end
    else
      send_resp(conn, 404, "")
    end
  rescue
    Plug.Static.InvalidPathError -> send_resp(conn, 404, "")
  end

  defp normalize_gzip(conn) do
    accepts_gzip =
      conn
      |> get_req_header("accept-encoding")
      |> Enum.flat_map(&String.split(&1, ","))
      |> Enum.any?(&accepts_gzip?/1)

    put_req_header(conn, "accept-encoding", if(accepts_gzip, do: "gzip", else: "identity"))
  end

  defp accepts_gzip?(coding) do
    case coding |> String.trim() |> String.downcase() |> String.split(~r/\s*;\s*q=/, parts: 2) do
      ["gzip"] ->
        true

      ["gzip", quality] ->
        case Float.parse(quality) do
          {q, ""} -> q > 0.0 && q <= 1.0
          _ -> false
        end

      _ ->
        false
    end
  end
end
