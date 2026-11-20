defmodule RepoCityWeb.ReviewParty do
  import Plug.Conn
  import RepoCityWeb.Router, only: [json: 3]
  alias RepoCity.GitHub.Cookie
  alias RepoCity.ReviewParty.{Access, Protocol, Relay, Room}

  @disabled "Review parties are not enabled on this server."
  @unknown {:error, 404, "Unknown party endpoint."}
  @verbs %{
    "invites" => :invite,
    "join" => :join,
    "renew" => :renew,
    "leave" => :leave,
    "remove" => :remove,
    "close" => :close,
    "revoke-invite" => :revoke_invite,
    "events" => :event,
    "socket" => :socket
  }

  def call(conn) do
    conn =
      conn
      |> put_resp_header("cache-control", "private, no-store")
      |> put_resp_header("referrer-policy", "no-referrer")

    enabled = Application.get_env(:repo_city, :review_parties_enabled, false)

    case conn.path_info do
      ["party", "config"] when conn.method == "GET" ->
        json(conn, 200, %{enabled: enabled, layout_version: 1})

      _ when not enabled ->
        json(conn, 503, %{error: @disabled})

      _ ->
        handle(conn)
    end
  end

  defp handle(conn) do
    start = System.monotonic_time()

    with {:ok, action, room_id} <- route(conn),
         {:ok, bearer} <- bearer(conn),
         :ok <- check_origin(conn, action, bearer),
         {:ok, input, conn} <- input(conn) do
      resolved =
        if action == :socket do
          socket(conn, room_id, bearer)
        else
          reply(conn, dispatch(action, room_id, input, conn, bearer))
        end

      measure(start, action, resolved)
    else
      {:error, _, _} = error ->
        measure(start, :invalid, reply(conn, error))
    end
  end

  defp dispatch(:create, _, input, conn, bearer) do
    credential = github_credential(conn, bearer)

    with {:ok, comparison} <- Protocol.comparison(input["comparison"]),
         {:ok, actor} <- Access.account(credential),
         {:ok, pinned, repository_id} <- Access.pin(comparison, credential) do
      Room.create(pinned, repository_id, actor) |> admission_response()
    end
  end

  defp dispatch(action, room_id, input, conn, bearer) when action in [:join, :renew] do
    credential = github_credential(conn, bearer)
    invite = input["invite"]

    with {:ok, actor} <- Access.account(credential),
         {:ok, room} <- Room.find(room_id),
         {:ok, comparison, repository_id} <-
           Room.command(room, {:comparison, action, actor.id, invite}),
         :ok <- verify_admission(room, action, actor, comparison, repository_id, credential) do
      Room.command(room, {:admit, action, actor, invite}) |> admission_response()
    end
  end

  defp dispatch(action, room_id, input, conn, bearer) do
    with {:ok, room} <- Room.find(room_id),
         {:ok, principal} <- principal(conn, bearer) do
      apply_member_action(room, principal, action, input, conn, bearer, room_id)
    end
  end

  defp apply_member_action(room, principal, :invite, input, conn, bearer, id) do
    with :ok <- Room.command(room, {:host_only, principal}),
         {:ok, target} <- Access.invitee(input["login"], invite_credential(conn, bearer)),
         {:ok, invitation} <- Room.command(room, {:member, principal, {:invite, target}}) do
      {:ok,
       %{
         invite_url: origin() <> "/?party=" <> id <> "#invite=" <> invitation.token,
         login: invitation.login
       }}
    end
  end

  defp apply_member_action(room, principal, :poll, _input, conn, _, _) do
    with {:ok, cursor} <- Protocol.cursor(fetch_query_params(conn).query_params["after"]) do
      Room.command(room, {:member, principal, {:poll, cursor}})
    end
  end

  defp apply_member_action(room, principal, :event, input, _, _, _) do
    with {:ok, event} <- Protocol.message(input) do
      Room.command(room, {:member, principal, {:event, event}})
    end
  end

  defp apply_member_action(room, principal, action, input, _, _, _) do
    command =
      case action do
        :remove -> {:remove, input["member_id"]}
        :revoke_invite -> {:revoke_invite, input["invite"]}
        other -> other
      end

    Room.command(room, {:member, principal, command})
  end

  defp verify_admission(room, action, actor, comparison, repository_id, credential) do
    case Access.verify(comparison, repository_id, credential) do
      {:error, status, _message} = error when action == :renew and status in [401, 403, 409] ->
        Room.command(room, {:member, {:account, actor.id}, :leave})
        error

      result ->
        result
    end
  end

  defp admission_response({:ok, body}) do
    {:ok,
     Map.merge(body, %{
       lease_ms: Room.lease_ms(),
       ice_servers: Relay.ice_servers(body.room.id, body.member.id)
     })}
  end

  defp admission_response(error), do: error

  defp route(%{path_info: ["party", "rooms"], method: "POST"}), do: {:ok, :create, nil}
  defp route(%{path_info: ["party", "rooms"]}), do: method_error()

  defp route(%{path_info: ["party", "rooms", id, action], method: method}) do
    case {Protocol.room_id?(id), @verbs[action], method} do
      {false, _, _} -> @unknown
      {_, nil, _} -> @unknown
      {true, :event, "GET"} -> {:ok, :poll, id}
      {true, :socket, "GET"} -> {:ok, :socket, id}
      {true, allowed, "POST"} when allowed != :socket -> {:ok, allowed, id}
      _ -> method_error()
    end
  end

  defp route(_), do: @unknown

  defp method_error, do: {:error, 405, "Method not allowed."}

  defp bearer(conn) do
    case get_req_header(conn, "authorization") do
      [] ->
        {:ok, nil}

      ["Bearer " <> token] when byte_size(token) in 1..2048 ->
        if Regex.match?(~r/\A[A-Za-z0-9._~-]+\z/, token),
          do: {:ok, token},
          else: {:error, 401, "Invalid room credential."}

      _ ->
        {:error, 401, "Invalid room credential."}
    end
  end

  defp check_origin(conn, action, bearer) do
    allowed = origin()

    case get_req_header(conn, "origin") do
      [^allowed] -> :ok
      [] when action != :socket and is_binary(bearer) -> :ok
      _ -> {:error, 403, "Invalid origin."}
    end
  end

  defp input(%{method: "GET"} = conn), do: {:ok, %{}, conn}

  defp input(conn) do
    limit = Protocol.max_body()

    case read_body(conn, length: limit, read_length: limit) do
      {:ok, body, conn} when byte_size(body) <= limit ->
        case Jason.decode(body) do
          {:ok, value} when is_map(value) -> {:ok, value, conn}
          _ -> {:error, 400, "Invalid JSON."}
        end

      {:error, _} ->
        {:error, 400, "Could not read the room request."}

      _ ->
        {:error, 413, "Message too large."}
    end
  end

  defp principal(_conn, "rcparty_" <> _ = token) do
    if Protocol.token?(token),
      do: {:ok, {:room_token, Protocol.digest(token)}},
      else: {:error, 401, "Invalid room credential."}
  end

  defp principal(conn, bearer) do
    with {:ok, actor} <- Access.account(github_credential(conn, bearer)) do
      {:ok, {:account, actor.id}}
    end
  end

  defp github_credential(conn, bearer) do
    case bearer do
      nil -> cookie_credential(conn)
      _ -> bearer
    end
  end

  defp invite_credential(conn, "rcparty_" <> _), do: cookie_credential(conn)
  defp invite_credential(conn, bearer), do: github_credential(conn, bearer)

  defp cookie_credential(conn) do
    with secret when is_binary(secret) and secret != "" <-
           Application.get_env(:repo_city, :github_client_secret),
         %{"token" => token} when is_binary(token) <- Cookie.session(conn, secret) do
      token
    else
      _ -> nil
    end
  end

  defp socket(conn, id, bearer) do
    with :ok <- upgrade_requested(conn),
         {:ok, room} <- Room.find(id),
         {:ok, credential, protocol} <- socket_credential(conn, bearer),
         {:ok, principal} <- principal(conn, credential),
         {:ok, _id} <- Room.command(room, {:authorize, principal}) do
      conn =
        if protocol, do: put_resp_header(conn, "sec-websocket-protocol", protocol), else: conn

      conn
      |> WebSockAdapter.upgrade(RepoCityWeb.ReviewPartySocket, {room, principal},
        max_frame_size: Protocol.max_body(),
        timeout: 120_000
      )
      |> halt()
    else
      {:error, _, _} = error -> reply(conn, error)
    end
  rescue
    WebSockAdapter.UpgradeError -> reply(conn, {:error, 400, "Invalid WebSocket upgrade."})
  end

  defp upgrade_requested(conn) do
    if Enum.any?(get_req_header(conn, "upgrade"), &(String.downcase(&1) == "websocket")),
      do: :ok,
      else: {:error, 426, "WebSocket upgrade required."}
  end

  defp socket_credential(conn, bearer) do
    protocols =
      conn
      |> get_req_header("sec-websocket-protocol")
      |> Enum.flat_map(&String.split(&1, ","))
      |> Enum.map(&String.trim/1)

    case protocols do
      [] ->
        {:ok, bearer, nil}

      ["repo-city-party", token] ->
        if Protocol.token?(token),
          do: {:ok, token, "repo-city-party"},
          else: {:error, 401, "Invalid room credential."}

      _ ->
        {:error, 400, "Invalid room protocol."}
    end
  end

  defp origin, do: Application.fetch_env!(:repo_city, :public_origin)

  defp reply(conn, {:ok, body}), do: json(conn, 200, body)
  defp reply(conn, {:error, status, message}), do: json(conn, status, %{error: message})

  defp measure(start, action, conn) do
    duration = System.monotonic_time() - start

    :telemetry.execute([:repo_city, :party, :request], %{duration: duration}, %{
      action: action,
      status: conn.status || 101
    })

    conn
  end
end
