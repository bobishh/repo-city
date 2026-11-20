defmodule RepoCityWeb.Router do
  import Plug.Conn
  alias RepoCityWeb.GitHub

  def init(options), do: options

  def call(conn, _options) do
    conn = put_resp_header(conn, "x-repo-city-backend", "vps")

    conn =
      case Application.get_env(:repo_city, :revision) do
        nil -> conn
        revision -> put_resp_header(conn, "x-repo-city-revision", revision)
      end

    dispatch(conn)
  end

  defp dispatch(%{path_info: ["github" | _]} = conn), do: GitHub.call(conn)

  defp dispatch(%{path_info: ["party", "config"], method: "GET"} = conn) do
    conn
    |> put_resp_header("cache-control", "no-store")
    |> json(200, %{enabled: false, layout_version: 1})
  end

  defp dispatch(%{path_info: ["party" | _]} = conn) do
    json(conn, 503, %{error: "Review parties are not enabled on this server."})
  end

  defp dispatch(%{method: method} = conn) when method not in ["GET", "HEAD"] do
    conn |> put_resp_header("allow", "GET, HEAD") |> send_resp(405, "")
  end

  defp dispatch(%{path_info: ["up"]} = conn) do
    conn |> put_resp_header("cache-control", "no-store") |> send_resp(200, "")
  end

  defp dispatch(conn), do: RepoCityWeb.Static.call(conn)

  def json(conn, status, body) do
    conn |> put_resp_content_type("application/json") |> send_resp(status, Jason.encode!(body))
  end
end
