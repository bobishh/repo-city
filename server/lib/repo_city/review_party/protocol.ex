defmodule RepoCity.ReviewParty.Protocol do
  @name ~r/\A[\w.-]{1,100}\z/
  @id ~r/\A[0-9]{1,20}\z/
  @sha ~r/\A(?:[a-f0-9]{40}|[a-f0-9]{64})\z/i
  @token ~r/\Arcparty_[a-f0-9]{64}\z/
  @safe_integer 9_007_199_254_740_991
  @voice %{enabled: false, speaking: false, agent_input: false, share_with_agents: false}
  @max_body 32_768

  def max_body, do: @max_body
  def mute_state, do: @voice
  def identity?(value), do: matches?(value, @id)
  def name?(value), do: matches?(value, @name) && value not in [".", ".."]
  def login?(value), do: matches?(value, ~r/\A[\w-]{1,100}\z/)
  def token?(value), do: matches?(value, @token)
  def token, do: "rcparty_" <> Base.encode16(:crypto.strong_rand_bytes(32), case: :lower)
  def digest(value), do: :crypto.hash(:sha256, value)

  def room_id do
    <<first::binary-4, second::binary-2, _::4, third::12, _::2, fourth::14, last::binary-6>> =
      :crypto.strong_rand_bytes(16)

    Enum.map_join(
      [first, second, <<4::4, third::12>>, <<2::2, fourth::14>>, last],
      "-",
      fn part ->
        Base.encode16(part, case: :lower)
      end
    )
  end

  def room_id?(value),
    do: matches?(value, ~r/\A[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\z/)

  def cursor(value) when is_integer(value) and value >= 0 and value <= @safe_integer,
    do: {:ok, value}

  def cursor(nil), do: {:ok, 0}

  def cursor(value) when is_binary(value) and byte_size(value) <= 16 do
    case Integer.parse(value) do
      {number, ""} -> cursor(number)
      _ -> invalid("Invalid cursor.")
    end
  end

  def cursor(_), do: invalid("Invalid cursor.")

  def comparison(
        %{
          "owner" => owner,
          "repo" => repo,
          "head_sha" => head,
          "base_sha" => base,
          "layout_version" => 1
        } = value
      ) do
    pr = value["pr_number"]

    if name?(owner) && name?(repo) && matches?(head, @sha) && matches?(base, @sha) &&
         (is_nil(pr) || (is_integer(pr) && pr > 0 && pr <= @safe_integer)) do
      comparison = %{
        owner: owner,
        repo: repo,
        head_sha: String.downcase(head),
        base_sha: String.downcase(base),
        layout_version: 1
      }

      {:ok, if(pr, do: Map.put(comparison, :pr_number, pr), else: comparison)}
    else
      invalid("Invalid comparison.")
    end
  end

  def comparison(_), do: invalid("Invalid comparison.")

  def comparison_id(repository_id, comparison) do
    [repository_id, comparison.base_sha, comparison.head_sha, comparison.layout_version]
    |> Enum.join(":")
    |> digest()
    |> Base.encode16(case: :lower)
  end

  def message(%{"type" => "pose", "seq" => seq, "position" => position, "forward" => forward})
      when is_integer(seq) and seq >= 0 and seq <= @safe_integer do
    if vector?(position) && vector?(forward),
      do: {:ok, %{type: "pose", pose_seq: seq, position: position, forward: forward}},
      else: invalid_message()
  end

  def message(%{"type" => "focus", "focus" => nil}), do: {:ok, %{type: "focus", focus: nil}}

  def message(%{"type" => type} = value) when type in ["focus", "cue"] do
    with {:ok, anchor} <- anchor(value[type]),
         true <- type == "focus" || Map.has_key?(anchor, :line) do
      case type do
        "focus" -> {:ok, %{type: "focus", focus: anchor}}
        "cue" -> {:ok, %{type: "cue", cue: anchor}}
      end
    else
      _ -> invalid_message()
    end
  end

  def message(%{"type" => "voice"} = value) do
    enabled = value["enabled"] == true

    {:ok,
     %{
       type: "voice",
       enabled: enabled,
       speaking: enabled && value["speaking"] == true,
       agent_input: value["agent_input"] == true,
       share_with_agents: value["share_with_agents"] == true
     }}
  end

  def message(%{"type" => "signal", "to" => to, "signal" => value}) do
    with true <- identity?(to),
         {:ok, signal} <- signal(value) do
      {:ok, %{type: "signal", to: to, signal: signal}}
    else
      _ -> invalid_message()
    end
  end

  def message(_), do: invalid_message()

  defp anchor(%{"path" => path, "side" => side, "face" => face} = value)
       when is_binary(path) and byte_size(path) <= 16_384 and side in ["before", "after"] and
              face in ["source", "diff", "blame"] do
    line = value["line"]
    last = value["end_line"]

    if String.length(path) <= 4096 && !Regex.match?(~r/[\x00-\x1f\\]/, path) &&
         Enum.all?(String.split(path, "/"), &(&1 not in ["", ".", ".."])) &&
         (is_nil(line) || line?(line)) &&
         (is_nil(last) || (line?(last) && line?(line) && last >= line)) do
      anchor = %{path: path, side: side, face: face}
      anchor = if line, do: Map.put(anchor, :line, line), else: anchor
      {:ok, if(last, do: Map.put(anchor, :end_line, last), else: anchor)}
    else
      :error
    end
  end

  defp anchor(_), do: :error

  defp signal(%{"type" => type, "sdp" => value})
       when type in ["offer", "answer"] and is_binary(value) and byte_size(value) <= 20_000,
       do: {:ok, %{type: type, sdp: value}}

  defp signal(%{"type" => "ice", "candidate" => nil}),
    do: {:ok, %{type: "ice", candidate: nil}}

  defp signal(%{"type" => "ice", "candidate" => %{"candidate" => candidate} = value})
       when is_binary(candidate) and byte_size(candidate) <= 2048 do
    mid = value["sdpMid"]
    index = value["sdpMLineIndex"]

    if (is_nil(mid) || (is_binary(mid) && byte_size(mid) <= 100)) &&
         (is_nil(index) || (is_integer(index) && index >= 0 && index <= 65535)) do
      {:ok, %{type: "ice", candidate: %{candidate: candidate, sdpMid: mid, sdpMLineIndex: index}}}
    else
      :error
    end
  end

  defp signal(_), do: :error

  defp matches?(value, regex), do: is_binary(value) && Regex.match?(regex, value)
  defp line?(value), do: is_integer(value) && value in 1..10_000_000

  defp vector?([_, _, _] = value),
    do: Enum.all?(value, &(is_number(&1) && abs(&1) <= 100_000_000))

  defp vector?(_), do: false
  defp invalid(message), do: {:error, 400, message}
  defp invalid_message, do: invalid("Invalid room message.")
end
