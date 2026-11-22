defmodule RepoCityWeb.Static do
  import Plug.Conn
  import Plug.HTML, only: [html_escape: 1]

  def deployment_index!(_root, configuration) when configuration in [nil, ""], do: nil

  def deployment_index!(root, configuration) do
    with {:ok, %{"src" => src} = script} when is_binary(src) <- Jason.decode(configuration),
         {:ok, %URI{scheme: "https", host: host, userinfo: nil}} when host not in [nil, ""] <-
           URI.new(src),
         data when is_map(data) <- Map.get(script, "data", %{}) do
      attributes = Enum.map_join(data, &data_attribute!/1)

      tag =
        ~s(  <script defer id="deployment-browser-script" src="#{html_escape(src)}" referrerpolicy="no-referrer"#{attributes}></script>\n)

      html = File.read!(Path.join(root, "index.html"))

      case String.split(html, "</head>", parts: 2) do
        [head, body] ->
          index = head <> tag <> "</head>" <> body
          etag = Base.url_encode64(:crypto.hash(:sha256, index), padding: false)
          {index, ~s("#{etag}")}

        _ ->
          raise ArgumentError, "Missing </head> in the deployment index"
      end
    else
      _ ->
        raise ArgumentError,
              "BROWSER_SCRIPT must contain an HTTPS src and optional data attributes"
    end
  end

  def call(conn) do
    root = Application.get_env(:repo_city, :static_root)
    conn = normalize_gzip(conn)
    path_info = if conn.path_info == [], do: ["index.html"], else: conn.path_info

    deployment_index = Application.get_env(:repo_city, :deployment_index)

    cond do
      path_info == ["index.html"] and deployment_index != nil ->
        send_index(conn, deployment_index)

      root ->
        send_static(conn, root, path_info)

      true ->
        send_resp(conn, 404, "")
    end
  rescue
    Plug.Static.InvalidPathError -> send_resp(conn, 404, "")
  end

  defp data_attribute!({name, value}) do
    if is_binary(value) and Regex.match?(~r/\A[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*\z/, name) do
      ~s( data-#{name}="#{html_escape(value)}")
    else
      raise ArgumentError, "Invalid data attribute in BROWSER_SCRIPT"
    end
  end

  defp send_index(conn, {index, etag}) do
    conn =
      conn
      |> put_resp_content_type("text/html")
      |> put_resp_header("cache-control", "no-cache")
      |> put_resp_header("etag", etag)
      |> put_resp_header("x-content-type-options", "nosniff")

    cond do
      etag in get_req_header(conn, "if-none-match") ->
        send_resp(conn, 304, "")

      conn.method == "HEAD" ->
        conn
        |> put_resp_header("content-length", Integer.to_string(byte_size(index)))
        |> send_resp(200, "")

      true ->
        send_resp(conn, 200, index)
    end
  end

  defp send_static(conn, root, path_info) do
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
