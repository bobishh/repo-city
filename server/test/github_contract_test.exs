defmodule RepoCity.GitHubContractTest do
  use ExUnit.Case, async: false
  import Plug.Conn
  import Plug.Test

  @object "/github/public/repos/Owner/Repo/git/blobs/" <> String.duplicate("a", 40)
  @origin "https://repo-city.meta-uber-engineer.dev"
  @session "__Host-repo-city"

  setup do
    RepoCity.GitHub.PublicCache.clear()
    Req.Test.set_req_test_to_shared()
    :ok
  end

  test "public objects are revalidated anonymously and vanish when upstream revokes access" do
    owner = self()

    Req.Test.expect(:github, fn conn ->
      assert conn.request_path ==
               String.replace_prefix(String.downcase(@object), "/github/public", "")

      assert get_req_header(conn, "authorization") == []
      assert get_req_header(conn, "cookie") == []
      assert get_req_header(conn, "if-none-match") == []
      send(owner, :anonymous)

      conn
      |> public_headers()
      |> put_resp_content_type("application/json")
      |> send_resp(200, ~s({"content":"public"}))
    end)

    first =
      request(:get, @object, [
        {"cookie", @session <> "=secret"},
        {"authorization", "Bearer ignored"}
      ])

    assert first.status == 200
    assert get_resp_header(first, "x-repo-city-cache") == ["MISS"]
    assert get_resp_header(first, "cache-control") == ["no-store"]
    assert_received :anonymous

    Req.Test.expect(:github, fn conn ->
      assert get_req_header(conn, "if-none-match") == [~s("v1")]
      conn |> public_headers() |> send_resp(304, "")
    end)

    hit = request(:get, @object)
    assert get_resp_header(hit, "x-repo-city-cache") == ["REVALIDATED"]
    assert hit.resp_body == first.resp_body

    Req.Test.expect(:github, fn conn -> send_resp(conn, 404, "") end)
    assert request(:get, @object).status == 404

    Req.Test.expect(:github, fn conn ->
      assert get_req_header(conn, "if-none-match") == []
      conn |> public_headers() |> send_resp(304, "")
    end)

    assert request(:get, @object).status == 404
  end

  test "private or credential-bearing upstream responses cannot populate the public cache" do
    for headers <- [[{"cache-control", "public, private"}], [{"set-cookie", "token=bad"}]] do
      Req.Test.expect(:github, fn conn ->
        conn = conn |> public_headers() |> put_resp_content_type("application/json")

        conn =
          Enum.reduce(headers, conn, fn {key, value}, acc -> put_resp_header(acc, key, value) end)

        send_resp(conn, 200, ~s({"content":"private"}))
      end)

      result = request(:get, @object)
      assert result.status == 404
      refute result.resp_body =~ ~s("private")
    end
  end

  test "health and disabled rooms are served by Phoenix" do
    assert request(:get, "/up").status == 200

    assert Jason.decode!(request(:get, "/party/config").resp_body) == %{
             "enabled" => false,
             "layout_version" => 1
           }
  end

  test "OAuth login uses configured origin and host-only PKCE cookie" do
    response = request(:get, "/github/login?return=%2F%3Frepo%3Downer%252Frepo")
    assert response.status == 303
    [location] = get_resp_header(response, "location")
    url = URI.parse(location)
    assert url.host == "github.com"
    params = URI.decode_query(url.query)
    assert params["redirect_uri"] == @origin <> "/github/callback"
    assert params["code_challenge_method"] == "S256"
    [cookie] = get_resp_header(response, "set-cookie")
    assert cookie =~ "__Host-repo-city-oauth="
    assert cookie =~ "HttpOnly"
    assert String.downcase(cookie) =~ "; secure"
    assert cookie =~ "SameSite=Lax"
    refute cookie =~ "Domain="
  end

  test "existing Node session survives migration and does not leak token in response" do
    {encoded, 0} = System.cmd("node", [Path.expand("fixtures/node_session.mjs", __DIR__)])

    response =
      request(:get, "/github/session", [{"cookie", @session <> "=" <> String.trim(encoded)}])

    assert response.status == 200

    assert Jason.decode!(response.resp_body) == %{
             "enabled" => true,
             "login" => "reviewer",
             "installUrl" => "https://github.com/apps/repo-city/installations/new"
           }

    assert get_resp_header(response, "cache-control") == ["private, no-store"]
    refute response.resp_body =~ "ghu_"
  end

  test "OAuth callback accepts a pre-migration flow and renews the encrypted session" do
    {encoded, 0} = System.cmd("node", [Path.expand("fixtures/node_session.mjs", __DIR__), "flow"])
    cookie = [{"cookie", "__Host-repo-city-oauth=" <> String.trim(encoded)}]

    failed = request(:get, "/github/callback?code=ok&state=wrong", cookie)
    assert get_resp_header(failed, "location") == ["/?github=failed&migrate=1"]

    Req.Test.expect(:github, fn conn ->
      assert conn.host == "github.com"
      assert conn.request_path == "/login/oauth/access_token"
      {:ok, form, conn} = read_body(conn)
      assert URI.decode_query(form)["code_verifier"] == "test-verifier"
      Req.Test.json(conn, %{access_token: "ghu_callback", expires_in: 3600})
    end)

    Req.Test.expect(:github, fn conn ->
      assert conn.request_path == "/user"
      assert get_req_header(conn, "authorization") == ["Bearer ghu_callback"]
      Req.Test.json(conn, %{login: "fresh-reviewer"})
    end)

    connected = request(:get, "/github/callback?code=ok&state=test-state", cookie)
    assert connected.status == 303
    assert get_resp_header(connected, "location") == ["/?github=connected&migrate=1"]
    cookies = get_resp_header(connected, "set-cookie")
    session = Enum.find(cookies, &String.starts_with?(&1, @session <> "="))
    refute session =~ "ghu_callback"
    assert Enum.any?(cookies, &String.starts_with?(&1, "__Host-repo-city-oauth=;"))
    headers = [{"cookie", session |> String.split(";", parts: 2) |> hd()}]

    assert Jason.decode!(request(:get, "/github/session", headers).resp_body)["login"] ==
             "fresh-reviewer"
  end

  test "authenticated Git objects bypass the public cache and keep the upstream response private" do
    target = "/repos/owner/repo/git/blobs/" <> String.duplicate("a", 40)
    headers = session_headers()

    for content <- ["first", "second"] do
      Req.Test.expect(:github, fn conn ->
        assert conn.request_path == target
        assert get_req_header(conn, "authorization") == ["Bearer ghu_fake"]
        assert get_req_header(conn, "cookie") == []
        assert get_req_header(conn, "if-none-match") == []
        Req.Test.json(conn, %{content: content})
      end)

      response = request(:get, "/github/api" <> target, headers)
      assert response.status == 200
      assert get_resp_header(response, "cache-control") == ["private, no-store"]
      assert Jason.decode!(response.resp_body) == %{"content" => content}
    end

    assert request(:get, "/github/api/user", headers).status == 404
    assert request(:get, "/github/api" <> target).status == 401
  end

  test "blame uses a scoped GraphQL query and returns the range contract" do
    Req.Test.expect(:github, fn conn ->
      assert conn.request_path == "/graphql"
      assert conn.method == "POST"
      {:ok, body, conn} = read_body(conn)
      assert Jason.decode!(body)["variables"]["path"] == "src/code.rs"

      Req.Test.json(conn, %{
        data: %{repository: %{object: %{blame: %{ranges: [%{startingLine: 1, endingLine: 3}]}}}}
      })
    end)

    path =
      "/github/blame?owner=owner&repo=repo&sha=#{String.duplicate("a", 40)}&path=src%2Fcode.rs"

    result = request(:get, path, session_headers())

    assert Jason.decode!(result.resp_body) == %{
             "ranges" => [%{"startingLine" => 1, "endingLine" => 3}]
           }
  end

  test "review comments retain optimistic revision check before publishing" do
    Req.Test.expect(:github, fn conn ->
      assert conn.request_path == "/repos/owner/repo/pulls/31/comments"
      Req.Test.json(conn, [])
    end)

    Req.Test.expect(:github, fn conn ->
      assert conn.request_path == "/repos/owner/repo/pulls/31"
      Req.Test.json(conn, %{head: %{sha: String.duplicate("b", 40)}})
    end)

    body =
      Jason.encode!(%{
        id: "12345678-1234-1234-1234-1234567890ab",
        preset: "note",
        revision: String.duplicate("a", 40),
        path: "src/code.rs",
        before: false,
        start: 1,
        end: 2,
        text: "Review text"
      })

    headers = [{"origin", @origin}, {"content-type", "application/json"} | session_headers()]
    result = dispatch(conn(:post, "/github/review/owner/repo/31", body), headers)
    assert result.status == 409
    assert result.resp_body =~ "The pull request changed"
  end

  test "review writes are same-origin and scoped to current GitHub diff coordinates" do
    target = "/github/review/owner/repo/31"
    upstream = "/repos/owner/repo/pulls/31"
    revision = String.duplicate("a", 40)
    id = "12345678-1234-1234-1234-1234567890ab"

    body =
      Jason.encode!(%{
        id: id,
        preset: "note",
        revision: revision,
        path: "src/code.rs",
        before: false,
        start: 2,
        end: 3,
        text: "Review text",
        replacement: "  updated()"
      })

    headers = [{"content-type", "application/json"} | session_headers()]
    assert dispatch(conn(:post, target, body), headers).status == 403

    Req.Test.expect(:github, fn conn ->
      assert conn.request_path == upstream <> "/comments"
      Req.Test.json(conn, [])
    end)

    Req.Test.expect(:github, fn conn ->
      assert conn.request_path == upstream
      Req.Test.json(conn, %{head: %{sha: revision}})
    end)

    Req.Test.expect(:github, fn conn ->
      assert conn.request_path == upstream <> "/files"
      patch = "@@ -1,2 +1,3 @@\n fn test() {\n-  old()\n+  new()\n+}\n"
      Req.Test.json(conn, [%{filename: "src/code.rs", patch: patch}])
    end)

    Req.Test.expect(:github, fn conn ->
      assert conn.method == "POST"
      assert conn.request_path == upstream <> "/comments"
      {:ok, json, conn} = read_body(conn)
      decoded = Jason.decode!(json)
      assert decoded["commit_id"] == revision
      assert decoded["path"] == "src/code.rs"
      assert decoded["side"] == "RIGHT"
      assert decoded["start_line"] == 2
      assert decoded["line"] == 3
      assert decoded["body"] =~ "repo-city:v1:#{id}:note"
      assert decoded["body"] =~ "```suggestion\n  updated()\n```"
      Req.Test.json(conn, %{id: 33})
    end)

    result = dispatch(conn(:post, target, body), [{"origin", @origin} | headers])
    assert result.status == 201
    assert Jason.decode!(result.resp_body) == %{"comment" => %{"id" => 33}}
  end

  defp session_headers do
    {encoded, 0} = System.cmd("node", [Path.expand("fixtures/node_session.mjs", __DIR__)])
    [{"cookie", @session <> "=" <> String.trim(encoded)}]
  end

  defp request(method, path, headers \\ []) do
    dispatch(conn(method, path), headers)
  end

  defp dispatch(conn, headers) do
    conn = Enum.reduce(headers, conn, fn {key, value}, acc -> put_req_header(acc, key, value) end)
    RepoCityWeb.Endpoint.call(conn, RepoCityWeb.Endpoint.init([]))
  end

  defp public_headers(conn) do
    conn
    |> put_resp_header("cache-control", "public, max-age=60")
    |> put_resp_header("etag", ~s("v1"))
  end
end
