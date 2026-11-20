defmodule RepoCity.StaticContractTest do
  use ExUnit.Case, async: false
  import Plug.Conn
  import Plug.Test

  @wasm <<0, 97, 115, 109, 1, 0, 0, 0>>

  test "serves WASM in gzip and identity representations" do
    run_id = Base.url_encode64(:crypto.strong_rand_bytes(12), padding: false)
    root = Path.join(System.tmp_dir!(), "repo-city-static-#{run_id}")
    on_exit(fn -> File.rm_rf!(root) end)
    File.mkdir_p!(Path.join(root, "game"))
    File.write!(Path.join(root, "game/repo_city_bg.wasm"), @wasm)
    File.write!(Path.join(root, "game/repo_city_bg.wasm.gz"), :zlib.gzip(@wasm))
    Application.put_env(:repo_city, :static_root, root)
    Application.put_env(:repo_city, :revision, "fixture")

    on_exit(fn ->
      Application.delete_env(:repo_city, :static_root)
      Application.delete_env(:repo_city, :revision)
    end)

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

  defp request(path, encoding) do
    :get
    |> conn(path)
    |> put_req_header("accept-encoding", encoding)
    |> RepoCityWeb.Endpoint.call(RepoCityWeb.Endpoint.init([]))
  end
end
