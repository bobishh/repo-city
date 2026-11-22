defmodule RepoCity.StaticContractTest do
  use ExUnit.Case, async: false
  import Plug.Conn
  import Plug.Test

  @wasm <<0, 97, 115, 109, 1, 0, 0, 0>>

  test "serves an external browser script only in a configured deployment" do
    root = temporary_root()
    html = "<html><head></head><body>Repo City</body></html>"
    File.write!(Path.join(root, "index.html"), html)
    put_setting(:static_root, root)

    ordinary = request("/", "identity")
    assert ordinary.resp_body == html

    configuration =
      Jason.encode!(%{
        src: "https://collector.example/browser.js?v=1&build=2",
        data: %{"project" => "repo-city", "token" => ~s(example"key)}
      })

    put_setting(:deployment_index, RepoCityWeb.Static.deployment_index!(root, configuration))

    page = request("/", "gzip")
    assert page.status == 200
    assert page.resp_body =~ ~s(src="https://collector.example/browser.js?v=1&amp;build=2")
    assert page.resp_body =~ ~s(data-project="repo-city")
    assert page.resp_body =~ ~s(data-token="example&quot;key")
    assert page.resp_body =~ "</script>\n</head>"
    assert request("/index.html", "gzip").resp_body == page.resp_body
    assert get_resp_header(page, "content-type") == ["text/html; charset=utf-8"]
    assert get_resp_header(page, "cache-control") == ["no-cache"]
    [original_etag] = get_resp_header(ordinary, "etag")
    [deployment_etag] = get_resp_header(page, "etag")
    assert deployment_etag != original_etag
    assert request("/", "gzip", :get, [{"if-none-match", original_etag}]).status == 200
    assert request("/", "gzip", :get, [{"if-none-match", deployment_etag}]).status == 304
    assert request("/", "gzip", :head).resp_body == ""
    assert request("/missing", "gzip").status == 404
  end

  test "rejects a configured script URL outside HTTPS" do
    assert_raise ArgumentError, ~r/BROWSER_SCRIPT/, fn ->
      RepoCityWeb.Static.deployment_index!(
        temporary_root(),
        Jason.encode!(%{src: "javascript:alert(1)"})
      )
    end
  end

  test "serves WASM in gzip and identity representations" do
    root = temporary_root()
    File.mkdir_p!(Path.join(root, "game"))
    File.write!(Path.join(root, "game/repo_city_bg.wasm"), @wasm)
    File.write!(Path.join(root, "game/repo_city_bg.wasm.gz"), :zlib.gzip(@wasm))
    put_setting(:static_root, root)
    put_setting(:revision, "fixture")

    compressed = request("/game/repo_city_bg.wasm", "br, gzip")
    assert compressed.status == 200
    assert get_resp_header(compressed, "content-type") == ["application/wasm"]
    assert get_resp_header(compressed, "content-encoding") == ["gzip"]
    assert get_resp_header(compressed, "vary") == ["Accept-Encoding"]
    assert get_resp_header(compressed, "x-repo-city-backend") == ["vps"]
    assert get_resp_header(compressed, "x-repo-city-revision") == ["fixture"]
    assert :zlib.gunzip(compressed.resp_body) == @wasm

    identity = request("/game/repo_city_bg.wasm", "gzip;q=0, identity")
    assert identity.status == 200
    assert get_resp_header(identity, "content-encoding") == []
    assert identity.resp_body == @wasm

    assert request("/game/missing.wasm", "gzip").status == 404
  end

  defp temporary_root do
    run_id = Base.url_encode64(:crypto.strong_rand_bytes(12), padding: false)
    root = Path.join(System.tmp_dir!(), "repo-city-static-#{run_id}")
    File.mkdir_p!(root)
    on_exit(fn -> File.rm_rf!(root) end)
    root
  end

  defp put_setting(key, value) do
    saved = Application.fetch_env(:repo_city, key)
    Application.put_env(:repo_city, key, value)

    on_exit(fn ->
      case saved do
        {:ok, previous} -> Application.put_env(:repo_city, key, previous)
        :error -> Application.delete_env(:repo_city, key)
      end
    end)
  end

  defp request(path, encoding, method \\ :get, headers \\ []) do
    method
    |> conn(path)
    |> Map.put(:req_headers, headers)
    |> put_req_header("accept-encoding", encoding)
    |> RepoCityWeb.Endpoint.call(RepoCityWeb.Endpoint.init([]))
  end
end
