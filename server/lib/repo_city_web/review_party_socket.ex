defmodule RepoCityWeb.ReviewPartySocket do
  @behaviour WebSock
  alias RepoCity.ReviewParty.{Protocol, Room}

  @impl true
  def init({room, principal}) do
    case Room.command(room, {:subscribe, principal, self()}) do
      {:ok, id, events} ->
        ref = Process.monitor(room)
        {:push, Enum.map(events, &frame/1), {room, ref, id}}

      {:error, _, reason} ->
        {:stop, :normal, {4003, reason}, nil}
    end
  end

  @impl true
  def handle_in({raw, [opcode: :text]}, {room, _ref, id} = state) do
    with true <- byte_size(raw) <= Protocol.max_body(),
         {:ok, value} <- Jason.decode(raw),
         {:ok, event} <- Protocol.message(value),
         {:ok, _} <- Room.command(room, {:socket_event, id, self(), event}) do
      {:ok, state}
    else
      {:error, 410, reason} -> {:stop, :normal, {4003, reason}, state}
      {:error, _, reason} -> {:push, frame(%{type: "error", error: reason}), state}
      _ -> {:push, frame(%{type: "error", error: "Invalid room message."}), state}
    end
  end

  def handle_in(_, state), do: {:stop, :normal, {1003, "Room messages must be text"}, state}

  @impl true
  def handle_info({:party_event, event}, state), do: {:push, frame(event), state}

  def handle_info({:party_close, code, reason}, state),
    do: {:stop, :normal, {code, reason}, state}

  def handle_info({:DOWN, ref, :process, room, _reason}, {room, ref, _} = state),
    do: {:stop, :normal, {4003, "Room closed"}, state}

  def handle_info(_, state), do: {:ok, state}

  defp frame(message), do: {:text, Jason.encode_to_iodata!(message)}
end
