defmodule RepoCity.GitHub.PublicCache do
  use GenServer
  alias RepoCity.GitHub.Client

  @max_object_bytes 4 * 1024 * 1024
  @max_bytes 24 * 1024 * 1024
  @max_entries 128
  @max_in_flight 8
  @retention_ms 15 * 60_000

  def start_link(_options), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)
  def read(target), do: GenServer.call(__MODULE__, {:read, target}, 15_000)
  def clear, do: GenServer.call(__MODULE__, :clear)

  @impl true
  def init(_), do: {:ok, %{entries: %{}, bytes: 0, pending: %{}, paused_until: now()}}

  @impl true
  def handle_call(:clear, _from, state) do
    {:reply, :ok, %{state | entries: %{}, bytes: 0, paused_until: now()}}
  end

  def handle_call({:read, target}, from, state) do
    time = now()

    cond do
      time < state.paused_until ->
        {:reply, {:error, 429}, state}

      pending = state.pending[target] ->
        pending = %{pending | waiters: [from | pending.waiters]}
        {:noreply, put_in(state.pending[target], pending)}

      map_size(state.pending) >= @max_in_flight ->
        {:reply, {:error, 503}, state}

      true ->
        entry = state.entries[target]
        saved = if entry && entry.expires > time, do: entry
        task = Task.Supervisor.async_nolink(RepoCity.GitHub.Tasks, fn -> fetch(target, saved) end)
        timer = Process.send_after(self(), {:deadline, target, task.ref}, 12_000)
        pending = %{waiters: [from], task: task, timer: timer}
        {:noreply, put_in(state.pending[target], pending)}
    end
  end

  @impl true
  def handle_info({ref, result}, state) when is_reference(ref) do
    Process.demonitor(ref, [:flush])
    {:noreply, finish(state, ref, result)}
  end

  def handle_info({:DOWN, ref, :process, _pid, _reason}, state) do
    {:noreply, finish(state, ref, {:error, 503})}
  end

  def handle_info({:deadline, target, ref}, state) do
    case state.pending[target] do
      %{task: %{ref: ^ref} = task} ->
        Task.shutdown(task, :brutal_kill)
        {:noreply, finish(state, ref, {:error, 503})}

      _ ->
        {:noreply, state}
    end
  end

  defp finish(state, ref, result) do
    case Enum.find(state.pending, fn {_target, pending} -> pending.task.ref == ref end) do
      {target, pending} ->
        Process.cancel_timer(pending.timer)
        state = remove(%{state | pending: Map.delete(state.pending, target)}, target)
        {state, reply} = apply_result(state, target, result)
        Enum.each(pending.waiters, &GenServer.reply(&1, reply))
        state

      nil ->
        state
    end
  end

  defp apply_result(state, target, {:ok, label, body, etag}) do
    state = if etag, do: store(state, target, body, etag), else: state
    {state, {:ok, label, body}}
  end

  defp apply_result(state, _target, {:rate_limited, status, delay}) do
    {%{state | paused_until: max(state.paused_until, now() + delay)}, {:error, status}}
  end

  defp apply_result(state, _target, {:error, _} = error), do: {state, error}

  defp fetch(target, saved) do
    case Client.public_object(target, saved && saved.etag, &receive_chunk/2) do
      {:ok, %{status: 304} = response} when not is_nil(saved) ->
        if public?(response) && header(response, "etag") in [nil, saved.etag] do
          {:ok, "REVALIDATED", saved.body, saved.etag}
        else
          {:error, 404}
        end

      {:ok, %{status: 200} = response} ->
        object(response)

      {:ok, %{status: status} = response} ->
        cond do
          status == 429 || (status == 403 && header(response, "x-ratelimit-remaining") == "0") ->
            reset = integer(header(response, "x-ratelimit-reset")) - System.os_time(:second)
            retry = integer(header(response, "retry-after"))
            {:rate_limited, status, 1000 * max(60, max(reset, retry))}

          status in [403, 404, 503] ->
            {:error, status}

          true ->
            {:error, 404}
        end

      {:error, _reason} ->
        {:error, 503}
    end
  end

  defp receive_chunk({:data, chunk}, {request, response}) do
    next_size = byte_size(response.body) + byte_size(chunk)

    cond do
      response.status != 200 || !eligible?(response) ->
        {:halt, {request, response}}

      next_size > @max_object_bytes ->
        {:halt, {request, %{response | body: :too_large}}}

      true ->
        {:cont, {request, %{response | body: response.body <> chunk}}}
    end
  end

  defp object(response) do
    cond do
      !eligible?(response) ->
        {:error, 404}

      response.body == :too_large || byte_size(response.body) > @max_object_bytes ->
        {:error, 413}

      response.body == "" ->
        {:error, 404}

      true ->
        etag = header(response, "etag")
        etag = if etag && Regex.match?(~r/\A(?:W\/)?"[\x21\x23-\x7e]{1,256}"\z/, etag), do: etag
        {:ok, "MISS", response.body, etag}
    end
  end

  defp eligible?(response) do
    public?(response) &&
      header(response, "content-encoding") in [nil, "identity"] &&
      Regex.match?(
        ~r/\Aapplication\/(?:json|vnd\.github\+json)\b/i,
        header(response, "content-type") || ""
      )
  end

  defp public?(response) do
    directives =
      response
      |> Req.Response.get_header("cache-control")
      |> Enum.flat_map(&String.split(&1, ","))
      |> Enum.map(
        &(String.split(&1, "=", parts: 2)
          |> hd()
          |> String.trim()
          |> String.downcase())
      )

    "public" in directives && "private" not in directives && "no-store" not in directives &&
      Req.Response.get_header(response, "set-cookie") == []
  end

  defp header(response, name), do: response |> Req.Response.get_header(name) |> List.first()

  defp store(state, target, body, etag) do
    size = byte_size(body)
    state = evict(state, size)
    time = now()
    entry = %{body: body, etag: etag, size: size, expires: time + @retention_ms}
    %{state | entries: Map.put(state.entries, target, entry), bytes: state.bytes + size}
  end

  defp evict(state, size)
       when map_size(state.entries) < @max_entries and state.bytes + size <= @max_bytes,
       do: state

  defp evict(state, size) do
    {oldest, _entry} = Enum.min_by(state.entries, fn {_target, entry} -> entry.expires end)
    state |> remove(oldest) |> evict(size)
  end

  defp remove(state, target) do
    case Map.pop(state.entries, target) do
      {nil, _} -> state
      {entry, entries} -> %{state | entries: entries, bytes: state.bytes - entry.size}
    end
  end

  defp integer(value) do
    case Integer.parse(value || "") do
      {number, ""} -> number
      _ -> 0
    end
  end

  defp now, do: System.monotonic_time(:millisecond)
end
