defmodule RepoCity.ReviewParty.Room do
  use GenServer
  alias RepoCity.ReviewParty.Protocol

  @lease_ms 300_000
  @room_ms 86_400_000
  @host_grace_ms 60_000
  @signal_ms 30_000
  @cue_limit 32
  @chat_limit 32
  @signal_limit 128
  @signal_bytes 512 * 1024

  def lease_ms, do: @lease_ms

  def create(comparison, repository_id, actor) do
    id = Protocol.room_id()
    token = Protocol.token()

    case DynamicSupervisor.start_child(
           RepoCity.ReviewParty.Rooms,
           {__MODULE__, {id, comparison, repository_id, actor, Protocol.digest(token)}}
         ) do
      {:ok, pid} -> command(pid, {:initial, token})
      {:error, :max_children} -> error(429, "Room service is at capacity. Try again later.")
      {:error, _reason} -> error(503, "Room service unavailable.")
    end
  end

  def find(id) do
    case Registry.lookup(RepoCity.ReviewParty.Registry, id) do
      [{pid, _}] -> {:ok, pid}
      [] -> closed_error()
    end
  end

  def command(pid, action) do
    GenServer.call(pid, action)
  catch
    :exit, _ -> closed_error()
  end

  def child_spec({id, _, _, _, _} = args) do
    %{id: id, start: {__MODULE__, :start_link, [args]}, restart: :temporary}
  end

  def start_link({id, _, _, _, _} = args) do
    GenServer.start_link(__MODULE__, args,
      name: {:via, Registry, {RepoCity.ReviewParty.Registry, id}}
    )
  end

  @impl true
  def init({id, comparison, repository_id, host, hash}) do
    now = now()

    info = %{
      id: id,
      comparison: comparison,
      comparison_id: Protocol.comparison_id(repository_id, comparison),
      host_id: host.id,
      closed: false
    }

    state = %{
      info: info,
      repository_id: repository_id,
      expires: now + @room_ms,
      host_deadline: now + @host_grace_ms,
      members: %{host.id => membership(host, hash, now)},
      invites: %{},
      sequence: 0,
      sockets: %{},
      presence: %{},
      cues: [],
      chat: [],
      sketch: nil,
      signals: [],
      rate: %{},
      timer: nil
    }

    {:ok, schedule(state)}
  end

  @impl true
  def format_status(status), do: Map.take(status, [:reason]) |> Map.put(:state, :redacted)

  @impl true
  def handle_call(action, _from, state) do
    now = now()
    {state, reply} = state |> expire(now) |> run_live(action, now)

    if state.info.closed,
      do: {:stop, :normal, reply, state},
      else: {:reply, reply, schedule(state)}
  end

  @impl true
  def handle_info({:timeout, ref, :expire}, %{timer: {ref, _}} = state) do
    state = expire(%{state | timer: nil}, now())
    if state.info.closed, do: {:stop, :normal, state}, else: {:noreply, schedule(state)}
  end

  def handle_info({:DOWN, ref, :process, pid, _}, state) do
    case Enum.find(state.sockets, fn {_, socket} -> socket.ref == ref && socket.pid == pid end) do
      {id, _} ->
        state = %{
          state
          | sockets: Map.delete(state.sockets, id),
            presence: Map.delete(state.presence, id)
        }

        state =
          if id == state.info.host_id,
            do: %{state | host_deadline: now() + @host_grace_ms},
            else: state

        {:noreply, state |> roster_event() |> schedule()}

      nil ->
        {:noreply, state}
    end
  end

  def handle_info(_, state), do: {:noreply, state}

  defp run_live(%{info: %{closed: true}} = state, _action, _now), do: {state, closed_error()}
  defp run_live(state, action, now), do: execute(action, state, now)

  defp execute({:initial, token}, state, _now) do
    {state, admitted(state, state.info.host_id, token)}
  end

  defp execute({:comparison, kind, id, invite}, state, now) do
    with :ok <- admission(state, kind, id, invite, now) do
      {state, {:ok, state.info.comparison, state.repository_id}}
    else
      error -> {state, error}
    end
  end

  defp execute({:admit, kind, actor, invite}, state, now) do
    with :ok <- admission(state, kind, actor.id, invite, now) do
      token = Protocol.token()

      invites =
        if kind == :join && Protocol.token?(invite),
          do: Map.delete(state.invites, Protocol.digest(invite)),
          else: state.invites

      member = membership(actor, Protocol.digest(token), now)
      updated = %{state | members: Map.put(state.members, actor.id, member), invites: invites}

      if budget?(updated) do
        updated = roster_event(updated)
        {updated, admitted(updated, actor.id, token)}
      else
        {state, metadata_error()}
      end
    else
      error -> {state, error}
    end
  end

  defp execute({:authorize, principal}, state, _now) do
    {state, resolve(state, principal)}
  end

  defp execute({:host_only, principal}, state, _now) do
    reply =
      with {:ok, id} <- resolve(state, principal),
           :ok <- host_only(state, id) do
        :ok
      end

    {state, reply}
  end

  defp execute({:subscribe, principal, pid}, state, _now) do
    with {:ok, id} <- resolve(state, principal) do
      state = drop_socket(state, id, 4000, "Reconnected")
      socket = %{pid: pid, ref: Process.monitor(pid)}
      presence = Map.delete(state.presence, id)
      state = %{state | presence: presence}
      state = if id == state.info.host_id, do: %{state | host_deadline: nil}, else: state
      state = roster_event(state)
      {pending, others} = Enum.split_with(state.signals, &(&1.to == id))
      state = %{state | signals: others, sockets: Map.put(state.sockets, id, socket)}
      history = history(state, pending) |> Enum.map(&public_event/1)
      {state, {:ok, id, [snapshot(state) | history]}}
    else
      error -> {state, error}
    end
  end

  defp execute({:member, principal, action}, state, now) do
    with {:ok, id} <- resolve(state, principal) do
      member_command(action, state, id, now)
    else
      error -> {state, error}
    end
  end

  defp execute({:socket_event, id, pid, message}, state, now) do
    with {:ok, ^id, ^pid} <- resolve_socket(state, id, pid) do
      member_command({:event, message}, state, id, now)
    else
      error -> {state, error}
    end
  end

  defp member_command({:invite, target}, state, id, now) do
    with :ok <- host_only(state, id),
         true <- map_size(state.invites) < setting(:party_pending_invites, 256) do
      token = Protocol.token()
      invitation = %{id: target.id, expires: min(now + @room_ms, state.expires)}
      updated = %{state | invites: Map.put(state.invites, Protocol.digest(token), invitation)}

      if budget?(updated),
        do: {updated, {:ok, %{token: token, login: target.login}}},
        else: {state, metadata_error()}
    else
      false -> {state, error(429, "Too many pending invitations. Revoke an invitation first.")}
      error -> {state, error}
    end
  end

  defp member_command({:revoke_invite, token}, state, id, _now) do
    with :ok <- host_only(state, id),
         true <- Protocol.token?(token) do
      {%{state | invites: Map.delete(state.invites, Protocol.digest(token))},
       {:ok, %{revoked: true}}}
    else
      false -> {state, error(400, "Invalid invitation.")}
      error -> {state, error}
    end
  end

  defp member_command({:poll, cursor}, state, id, now) do
    state =
      if id == state.info.host_id && !Map.has_key?(state.sockets, id),
        do: %{state | host_deadline: now + @host_grace_ms},
        else: state

    poses = for %{pose: pose} <- Map.values(state.presence), do: pose
    {signals, others} = Enum.split_with(state.signals, &(&1.to == id))

    events = history(state, poses ++ signals) |> Enum.filter(&(&1.seq > cursor))

    {%{state | signals: others},
     {:ok,
      %{
        room: state.info,
        members: roster(state),
        cursor: state.sequence,
        events: Enum.map(events, &public_event/1)
      }}}
  end

  defp member_command({:event, message}, state, id, now) do
    {state, count} = count_event(state, id, now)

    cond do
      count > setting(:party_messages_per_second, 120) ->
        {state, error(429, "Room message budget exceeded. Reduce update rate.")}

      message.type == "signal" && !Map.has_key?(state.members, message.to) ->
        {state, {:ok, %{accepted: false, cursor: state.sequence}}}

      true ->
        accept_event(message, state, id, now)
    end
  end

  defp member_command(:leave, state, id, _now), do: end_membership(state, id)

  defp member_command(:close, state, id, _now) do
    case host_only(state, id) do
      :ok -> end_membership(state, id)
      error -> {state, error}
    end
  end

  defp member_command({:remove, target}, state, id, _now) do
    with :ok <- host_only(state, id),
         true <- Protocol.identity?(target) do
      end_membership(state, target)
    else
      false -> {state, error(400, "Invalid member.")}
      error -> {state, error}
    end
  end

  defp admission(state, :renew, id, _invite, _now) do
    with {:ok, _} <- resolve(state, {:account, id}), do: :ok
  end

  defp admission(state, :join, id, invite, now) do
    if Map.has_key?(state.members, id) ||
         (Protocol.token?(invite) &&
            match?(
              %{id: ^id, expires: expires} when expires > now,
              state.invites[Protocol.digest(invite)]
            )) do
      :ok
    else
      error(403, "Invitation is valid only for its intended account.")
    end
  end

  defp resolve(state, {:account, id}) do
    if Map.has_key?(state.members, id), do: {:ok, id}, else: membership_error()
  end

  defp resolve(state, {:room_token, hash}) do
    case Enum.find(state.members, fn {_, member} ->
           Plug.Crypto.secure_compare(member.token_hash, hash)
         end) do
      {id, _} -> {:ok, id}
      nil -> membership_error()
    end
  end

  defp resolve_socket(state, id, pid) do
    case state.sockets[id] do
      %{pid: ^pid} -> {:ok, id, pid}
      _ -> membership_error()
    end
  end

  defp accept_event(%{type: "pose"} = message, state, id, now) do
    presence = Map.get(state.presence, id, %{})
    last = Map.get(presence, :pose)

    if last && message.pose_seq <= last.pose_seq do
      {state, {:ok, %{accepted: false, cursor: state.sequence}}}
    else
      {state, event} = sequence(state, Map.put(message, :from, id))
      last_sent = Map.get(presence, :pose_sent)
      send_pose = is_nil(last_sent) || now - last_sent >= 100
      if send_pose, do: broadcast(state, event)

      presence =
        Map.merge(presence, %{pose: event, pose_sent: if(send_pose, do: now, else: last_sent)})

      accepted(put_in(state.presence[id], presence))
    end
  end

  defp accept_event(%{type: "signal"} = message, state, id, now) do
    {updated, event} = sequence(state, Map.put(message, :from, id))

    case updated.sockets[message.to] do
      %{pid: pid} ->
        deliver(pid, event)
        accepted(updated)

      nil ->
        bytes = byte_size(Jason.encode!(event))
        total = Enum.reduce(updated.signals, bytes, fn signal, acc -> acc + signal.bytes end)

        if length(updated.signals) < @signal_limit && total <= @signal_bytes do
          accepted(%{
            updated
            | signals: [
                Map.merge(event, %{expires: now + @signal_ms, bytes: bytes}) | updated.signals
              ]
          })
        else
          {state, error(429, "Voice signaling queue is full. Reconnect voice and retry.")}
        end
    end
  end

  defp accept_event(%{type: "chat", id: message_id} = message, state, id, _now) do
    case Enum.find(state.chat, &(&1.from == id && &1.id == message_id)) do
      nil ->
        {state, event} =
          sequence(state, Map.merge(message, %{from: id, login: state.members[id].login}))

        broadcast(state, event)
        accepted(%{state | chat: Enum.take([event | state.chat], @chat_limit)})

      %{text: text} when text == message.text ->
        accepted(state)

      _ ->
        {state, error(409, "This message ID already belongs to another message.")}
    end
  end

  defp accept_event(
         %{type: "sketch", title: title, source: source, format: format} = message,
         state,
         id,
         now
       ) do
    with :ok <- host_only(state, id) do
      case state.sketch do
        %{title: ^title, source: ^source, format: ^format} ->
          accepted(state)

        %{updated_at: previous} when now - previous < 1000 ->
          {state, error(429, "Wait a moment before updating the sketch again.")}

        _ ->
          {state, event} = sequence(state, Map.put(message, :from, id))
          broadcast(state, event)
          accepted(%{state | sketch: Map.put(event, :updated_at, now)})
      end
    else
      error -> {state, error}
    end
  end

  defp accept_event(message, state, id, _now) do
    {state, event} = sequence(state, Map.put(message, :from, id))

    state =
      case event do
        %{type: "focus", focus: focus} ->
          update_presence(state, id, :focus, focus)

        %{type: "voice"} ->
          update_presence(state, id, :voice, Map.take(event, Map.keys(Protocol.mute_state())))

        %{type: "cue"} ->
          %{state | cues: Enum.take([event | state.cues], @cue_limit)}
      end

    broadcast(state, event)
    accepted(state)
  end

  defp accepted(state), do: {state, {:ok, %{accepted: true, cursor: state.sequence}}}

  defp update_presence(state, id, field, value) do
    presence = Map.get(state.presence, id, %{}) |> Map.put(field, value)
    put_in(state.presence[id], presence)
  end

  defp end_membership(state, id) do
    if id == state.info.host_id do
      {close(state), {:ok, %{closed: true, left: id}}}
    else
      updated = state |> remove_member(id) |> roster_event()
      {updated, {:ok, %{closed: false, left: id}}}
    end
  end

  defp remove_member(state, id) do
    state = drop_socket(state, id, 4003, "Membership ended")

    %{
      state
      | members: Map.delete(state.members, id),
        presence: Map.delete(state.presence, id),
        rate: Map.delete(state.rate, id),
        invites: Map.reject(state.invites, fn {_, invite} -> invite.id == id end),
        signals: Enum.reject(state.signals, &(&1.from == id || &1.to == id))
    }
  end

  defp close(state) do
    state = put_in(state.info.closed, true)
    {state, event} = sequence(state, %{type: "closed", room: state.info})
    broadcast(state, event)

    Enum.each(state.sockets, fn {_, socket} ->
      send(socket.pid, {:party_close, 4003, "Room closed"})
    end)

    state
  end

  defp expire(state, now) do
    if state.expires <= now || (state.host_deadline && state.host_deadline <= now) ||
         state.members[state.info.host_id].lease_expires <= now do
      close(state)
    else
      expired = for {id, %{lease_expires: expires}} <- state.members, expires <= now, do: id
      state = Enum.reduce(expired, state, fn id, acc -> remove_member(acc, id) end)

      state = %{
        state
        | invites: Map.reject(state.invites, fn {_, invite} -> invite.expires <= now end),
          signals: Enum.filter(state.signals, &(&1.expires > now))
      }

      if expired == [], do: state, else: roster_event(state)
    end
  end

  defp schedule(state) do
    deadline =
      Enum.min(
        [state.expires, state.host_deadline] ++
          Enum.map(state.members, fn {_, member} -> member.lease_expires end) ++
          Enum.map(state.signals, & &1.expires),
        fn left, right -> right == nil || (left != nil && left <= right) end
      )

    case state.timer do
      {_, ^deadline} ->
        state

      previous ->
        if previous, do: Process.cancel_timer(elem(previous, 0))
        ref = :erlang.start_timer(max(0, deadline - now()), self(), :expire)
        %{state | timer: {ref, deadline}}
    end
  end

  defp roster_event(state) do
    {state, event} = sequence(state, snapshot(state))
    broadcast(state, event)
    state
  end

  defp snapshot(state),
    do: %{type: "roster", room: state.info, members: roster(state), seq: state.sequence}

  defp roster(state) do
    state.members
    |> Enum.map(fn {id, member} ->
      presence = Map.get(state.presence, id, %{})
      public = Map.take(member, [:id, :login, :avatar_url, :lease_expires])

      Map.merge(public, %{
        focus: presence[:focus],
        pose: presence[:pose],
        voice: Map.get(presence, :voice, Protocol.mute_state())
      })
    end)
    |> Enum.sort_by(& &1.id)
  end

  defp admitted(state, id, token) do
    members = roster(state)

    {:ok,
     %{
       room: state.info,
       member: Enum.find(members, &(&1.id == id)),
       members: members,
       room_token: token,
       cursor: state.sequence,
       events: Enum.map(history(state), &public_event/1),
       socket_url: "/party/rooms/#{state.info.id}/socket"
     }}
  end

  defp history(state, additional \\ []),
    do: Enum.sort_by(state.chat ++ state.cues ++ List.wrap(state.sketch) ++ additional, & &1.seq)

  defp drop_socket(state, id, code, reason) do
    case Map.pop(state.sockets, id) do
      {nil, _} ->
        state

      {%{pid: pid, ref: ref}, sockets} ->
        Process.demonitor(ref, [:flush])
        send(pid, {:party_close, code, reason})
        %{state | sockets: sockets}
    end
  end

  defp broadcast(state, event),
    do: Enum.each(state.sockets, fn {_, socket} -> deliver(socket.pid, event) end)

  defp deliver(pid, event) do
    case Process.info(pid, :message_queue_len) do
      {:message_queue_len, 128} -> send(pid, {:party_close, 1013, "Connection is too slow"})
      {:message_queue_len, count} when count > 128 -> :ok
      {:message_queue_len, count} when count >= 32 and event.type == "pose" -> :ok
      {:message_queue_len, _} -> send(pid, {:party_event, public_event(event)})
      nil -> :ok
    end
  end

  defp public_event(event), do: Map.drop(event, [:expires, :bytes, :updated_at])

  defp count_event(state, id, now) do
    {since, count} = Map.get(state.rate, id, {now, 0})
    {since, count} = if now - since >= 1000, do: {now, 1}, else: {since, count + 1}
    {put_in(state.rate[id], {since, count}), count}
  end

  defp sequence(state, event) do
    seq = state.sequence + 1
    {%{state | sequence: seq}, Map.put(event, :seq, seq)}
  end

  defp membership(actor, hash, now),
    do: Map.merge(actor, %{token_hash: hash, lease_expires: now + @lease_ms})

  defp budget?(state),
    do:
      :erlang.external_size({state.info, state.members, state.invites}) <=
        setting(:party_room_bytes, 131_072)

  defp setting(key, default), do: Application.get_env(:repo_city, key, default)
  defp now, do: System.system_time(:millisecond)
  defp host_only(%{info: %{host_id: id}}, id), do: :ok
  defp host_only(_, _), do: error(403, "Only the host can manage room membership.")
  defp closed_error, do: error(410, "Room is closed.")
  defp membership_error, do: error(403, "Room membership expired or was removed.")

  defp metadata_error,
    do:
      error(
        429,
        "Room metadata budget reached. Close unused memberships before inviting more reviewers."
      )

  defp error(status, message), do: {:error, status, message}
end
