defmodule RepoCity.ReviewPartyTest do
  use ExUnit.Case, async: false
  import Plug.Conn
  import Plug.Test

  @origin "https://repo-city.meta-uber-engineer.dev"
  @head String.duplicate("b", 40)
  @base String.duplicate("a", 40)
  @comparison %{
    owner: "owner",
    repo: "private",
    head_sha: @head,
    base_sha: @base,
    layout_version: 1
  }

  setup do
    Application.put_env(:repo_city, :review_parties_enabled, true)
    on_exit(fn -> Application.delete_env(:repo_city, :review_parties_enabled) end)
    Req.Test.set_req_test_to_shared()

    RepoCity.ReviewPartyTest.stub_github()
    :ok
  end

  test "invite belongs to one GitHub account, and private access is rechecked on join and renew" do
    assert %{"enabled" => true} = json(request(:get, "/party/config"))
    first = create()
    second = create()
    assert first["room"]["id"] != second["room"]["id"]
    assert first["room"]["comparison_id"] == second["room"]["comparison_id"]
    assert String.length(first["room"]["comparison_id"]) == 64

    path = room_path(first)

    invitation =
      request(:post, path <> "/invites", %{login: "guest"}, first["room_token"]) |> json()

    invite =
      invitation["invite_url"]
      |> URI.parse()
      |> Map.fetch!(:fragment)
      |> URI.decode_query()
      |> Map.fetch!("invite")

    for credential <- ["stranger", "lost-access"] do
      denied = request(:post, path <> "/join", %{invite: invite}, credential)
      assert denied.status == 403
      refute denied.resp_body =~ "private"
    end

    joined = request(:post, path <> "/join", %{invite: invite}, "guest") |> json()
    assert joined["member"]["id"] == "2"
    assert joined["room"]["comparison"]["head_sha"] == @head

    denied = request(:post, path <> "/renew", %{}, "lost-access")
    assert denied.status == 403
    assert request(:get, path <> "/events", nil, joined["room_token"]).status == 403
    assert request(:post, path <> "/join", %{invite: invite}, "guest").status == 403
  end

  test "presence is coalesced and directed WebRTC signals are consumed by the intended member" do
    first = create()
    path = room_path(first)
    second = join_guest(first)
    host = first["room_token"]
    guest = second["room_token"]
    cue = %{path: "src/main.rs", face: "blame", side: "after", line: 5, source: "private body"}

    for seq <- 1..6 do
      assert request(
               :post,
               path <> "/events",
               %{type: "pose", seq: seq, position: [seq, 0, 0], forward: [0, 0, -1]},
               host
             ).status == 200
    end

    assert request(
             :post,
             path <> "/events",
             %{type: "cue", cue: cue, draft: "private draft"},
             host
           ).status == 200

    assert request(
             :post,
             path <> "/events",
             %{type: "signal", to: "2", signal: %{type: "ice", candidate: nil}},
             host
           ).status == 200

    events = json(request(:get, path <> "/events", nil, host))["events"]
    refute Enum.any?(events, &(&1["type"] == "signal"))

    poll = request(:get, path <> "/events", nil, guest)
    refute poll.resp_body =~ "private body"
    refute poll.resp_body =~ "private draft"
    events = json(poll)["events"]
    assert [%{"position" => [6, 0, 0]}] = Enum.filter(events, &(&1["type"] == "pose"))
    assert [%{"from" => "1", "to" => "2"}] = Enum.filter(events, &(&1["type"] == "signal"))
    assert Enum.any?(events, &(&1["type"] == "cue"))

    refute Enum.any?(
             json(request(:get, path <> "/events", nil, guest))["events"],
             &(&1["type"] == "signal")
           )

    assert request(:post, path <> "/remove", %{member_id: "1"}, guest).status == 403
    assert request(:post, path <> "/remove", %{member_id: "2"}, host).status == 200
    assert request(:post, path <> "/events", %{type: "voice", enabled: true}, guest).status == 403
  end

  test "closing a room invalidates the links and credentials" do
    first = create()
    path = room_path(first)
    second = join_guest(first)
    assert json(request(:post, path <> "/close", %{}, first["room_token"]))["closed"]
    assert request(:get, path <> "/events", nil, second["room_token"]).status == 410
  end

  test "room members see the same attributed chat history and retries leave one message" do
    admitted = create()
    path = room_path(admitted)
    host = admitted["room_token"]
    message = %{type: "chat", id: "prompt-1", text: "  Check this diff.  ", from: "2"}

    assert request(:post, path <> "/events", message, "stranger").status == 403
    assert json(request(:post, path <> "/events", message, host))["accepted"]
    assert json(request(:post, path <> "/events", message, host))["accepted"]
    assert request(:post, path <> "/events", %{message | text: "  "}, host).status == 400

    guest = join_guest(admitted)

    assert [%{"type" => "chat", "from" => "1", "login" => "host", "text" => "Check this diff."}] =
             guest["events"]

    assert guest["events"] ==
             json(request(:get, path <> "/events", nil, guest["room_token"]))["events"]

    other = create()

    assert json(request(:get, room_path(other) <> "/events", nil, other["room_token"]))["events"] ==
             []
  end

  test "browser requests require the same origin and an authorized WebSocket upgrade" do
    assert request(
             :post,
             "/party/rooms",
             %{comparison: @comparison},
             "host",
             "https://other.example"
           ).status == 403

    first = create()
    path = room_path(first)

    assert request(:get, path <> "/socket", nil, first["room_token"], "https://other.example").status ==
             403

    assert request(:get, path <> "/socket", nil, first["room_token"], @origin).status == 426
  end

  test "only the host changes the sketch and reconnect sees the current sketch" do
    admitted = create()
    guest = join_guest(admitted)
    path = room_path(admitted)
    sketch = %{type: "sketch", title: "Flow", source: "flowchart LR\nA --> B"}

    assert request(:post, path <> "/events", sketch, guest["room_token"]).status == 403
    assert json(request(:post, path <> "/events", sketch, admitted["room_token"]))["accepted"]
    assert json(request(:post, path <> "/events", sketch, admitted["room_token"]))["accepted"]

    renewed = json(request(:post, path <> "/renew", %{}, "guest"))

    assert [
             %{
               "type" => "sketch",
               "title" => "Flow",
               "format" => "mermaid",
               "source" => "flowchart LR\nA --> B"
             }
           ] =
             renewed["events"]
  end

  defp create, do: request(:post, "/party/rooms", %{comparison: @comparison}, "host") |> json()
  defp room_path(room), do: "/party/rooms/" <> room["room"]["id"]

  defp join_guest(first) do
    path = room_path(first)

    invitation =
      request(:post, path <> "/invites", %{login: "guest"}, first["room_token"]) |> json()

    invite = URI.decode_query(URI.parse(invitation["invite_url"]).fragment)["invite"]
    request(:post, path <> "/join", %{invite: invite}, "guest") |> json()
  end

  defp request(method, path, body \\ nil, credential \\ nil, origin \\ nil) do
    conn = conn(method, path, if(body, do: Jason.encode!(body), else: ""))
    conn = if body, do: put_req_header(conn, "content-type", "application/json"), else: conn

    conn =
      if credential,
        do: put_req_header(conn, "authorization", "Bearer " <> credential),
        else: conn

    conn = if origin, do: put_req_header(conn, "origin", origin), else: conn
    RepoCityWeb.Endpoint.call(conn, RepoCityWeb.Endpoint.init([]))
  end

  defp json(conn) do
    assert conn.status == 200, conn.resp_body
    assert get_resp_header(conn, "cache-control") == ["private, no-store"]
    Jason.decode!(conn.resp_body)
  end

  def stub_github do
    Req.Test.stub(:github, fn conn ->
      credential = List.first(get_req_header(conn, "authorization"))
      refute String.starts_with?(credential || "", "Bearer rcparty_")

      case conn.path_info do
        ["user"] ->
          actor =
            case credential do
              "Bearer host" ->
                %{id: 1, login: "host"}

              value when value in ["Bearer guest", "Bearer lost-access"] ->
                %{id: 2, login: "guest"}

              "Bearer stranger" ->
                %{id: 3, login: "stranger"}
            end

          Req.Test.json(conn, actor)

        ["users", "guest"] ->
          Req.Test.json(conn, %{id: 2, login: "guest"})

        ["repos", "owner", "private"] when credential == "Bearer lost-access" ->
          send_resp(conn, 404, "")

        ["repos", "owner", "private"] when credential in ["Bearer host", "Bearer guest"] ->
          Req.Test.json(conn, %{id: 99, full_name: "owner/private", private: true})

        ["repos", "owner", "private", "commits", sha]
        when credential in ["Bearer host", "Bearer guest"] ->
          Req.Test.json(conn, %{sha: sha})
      end
    end)
  end
end
