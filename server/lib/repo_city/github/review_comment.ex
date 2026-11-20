defmodule RepoCity.GitHub.ReviewComment do
  @note_id ~r/\A[a-f\d]{8}(?:-[a-f\d]{4}){3}-[a-f\d]{12}\z/i
  @preset ~r/\A[a-z\d-]{1,80}\z/i
  @sha ~r/\A[a-f\d]{40}\z/i
  @metadata ~r/\n\n<!-- repo-city:v1:([a-f\d-]{36}):[a-z\d-]{1,80} -->\z/i
  @hunk ~r/\A@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/

  def publish(input, login, request) do
    with {:ok, draft} <- draft(input),
         :new <- existing(request, login, draft.id, 1),
         {:ok, pull, _response} <- read(request, ""),
         :ok <- check_revision(pull, draft.revision),
         {:ok, file} <- find_file(request, draft, 1),
         :ok <- check_range(file, draft) do
      write(request, file, draft)
    else
      {:existing, comment} -> {200, %{comment: comment}}
      error -> error
    end
  end

  def valid_file?(path) do
    is_binary(path) && String.length(path) <= 4096 && !Regex.match?(~r/[\x00-\x1f]/, path) &&
      Enum.all?(String.split(path, "/"), &(&1 not in ["", ".", ".."]))
  end

  defp draft(
         %{
           "id" => id,
           "revision" => revision,
           "path" => path,
           "before" => before,
           "start" => first,
           "end" => last,
           "preset" => preset,
           "text" => text
         } = input
       ) do
    file_level = Map.get(input, "fileLevel", false)
    replacement = Map.get(input, "replacement", :absent)

    cond do
      !matches?(id, @note_id) || !matches?(revision, @sha) || !matches?(preset, @preset) ||
        !valid_file?(path) || !is_boolean(before) || !is_boolean(file_level) ||
        !is_integer(first) || !is_integer(last) || first < 1 || last < first ||
        last > 9_007_199_254_740_991 || !is_binary(text) || String.length(text) > 6000 ||
          (replacement == :absent && String.trim(text) == "") ->
        error(400, "Invalid draft comment.")

      replacement != :absent &&
          (file_level || before || !is_binary(replacement) || replacement == "" ||
             String.length(replacement) > 8000) ->
        error(
          400,
          "A suggestion needs replacement code and lines on the current side of the diff."
        )

      true ->
        {:ok,
         %{
           id: id,
           revision: revision,
           path: path,
           before: before,
           first: first,
           last: last,
           preset: preset,
           text: String.trim(text),
           file_level: file_level,
           replacement: replacement
         }}
    end
  end

  defp draft(_), do: error(400, "Invalid draft comment.")

  defp read(request, suffix) do
    case request.(suffix, []) do
      {:ok, %{status: status, body: body} = response} when status in 200..299 ->
        case Jason.decode(body) do
          {:ok, data} -> {:ok, data, response}
          _ -> access_error(502)
        end

      {:ok, %{status: status}} ->
        access_error(status)

      {:error, _} ->
        access_error(502)
    end
  end

  defp existing(request, login, id, page) do
    case read(request, "/comments?sort=created&direction=desc&per_page=100&page=#{page}") do
      {:ok, comments, response} when is_list(comments) ->
        comment =
          Enum.find(comments, fn
            %{"user" => %{"login" => ^login}, "body" => body} when is_binary(body) ->
              match?([_, ^id], Regex.run(@metadata, body))

            _ ->
              false
          end)

        cond do
          comment ->
            {:existing, comment}

          !next?(response) ->
            :new

          page == 12 ->
            error(409, "Too many comments to check safely. Check GitHub before retrying.")

          true ->
            existing(request, login, id, page + 1)
        end

      {:ok, _value, _response} ->
        access_error(502)

      error ->
        error
    end
  end

  defp check_revision(%{"head" => %{"sha" => sha}}, sha), do: :ok

  defp check_revision(_pull, _sha) do
    error(409, "The pull request changed. Open the current revision and reselect the lines.")
  end

  defp find_file(request, draft, page) do
    case read(request, "/files?per_page=100&page=#{page}") do
      {:ok, files, response} when is_list(files) ->
        file =
          Enum.find(files, fn
            %{"filename" => filename} = file when is_binary(filename) ->
              filename == draft.path || (draft.before && file["previous_filename"] == draft.path)

            _ ->
              false
          end)

        cond do
          file ->
            {:ok, file}

          page < 30 && next?(response) ->
            find_file(request, draft, page + 1)

          true ->
            error(
              422,
              "That file is not in the current GitHub diff. Keep your draft or reselect a changed file."
            )
        end

      {:ok, _value, _response} ->
        access_error(502)

      error ->
        error
    end
  end

  defp check_range(_file, %{file_level: true}), do: :ok

  defp check_range(file, draft) do
    cond do
      contains_range?(file["patch"], draft.before, draft.first, draft.last) ->
        :ok

      draft.replacement != :absent ->
        error(422, "Select lines within one GitHub diff hunk. Your suggestion is still saved.")

      true ->
        {422,
         %{
           error:
             "That range is outside a single GitHub diff hunk. Keep it private, reselect the lines, or publish at file level.",
           fileLevel: true
         }}
    end
  end

  defp contains_range?(patch, before, first, last) when is_binary(patch) do
    changed = if before, do: ?-, else: ?+

    patch
    |> String.split("\n")
    |> Enum.reduce_while({0, 0, 0}, fn row, {line, count, matched} = acc ->
      case Regex.run(@hunk, row) do
        [_, left, left_count, right | right_count] ->
          start = if before, do: left, else: right
          length = if before, do: left_count, else: List.first(right_count)
          length = if length in [nil, ""], do: 1, else: String.to_integer(length)
          {:cont, {String.to_integer(start), length, 0}}

        nil ->
          case row do
            <<marker, _::binary>> when count > 0 and marker in [32, changed] ->
              matched = if first + matched == line, do: matched + 1, else: matched

              if matched > 0 && line == last,
                do: {:halt, true},
                else: {:cont, {line + 1, count - 1, matched}}

            _ ->
              {:cont, acc}
          end
      end
    end) == true
  end

  defp contains_range?(_patch, _before, _first, _last), do: false

  defp write(request, file, draft) do
    body = %{
      body: comment_body(draft),
      commit_id: draft.revision,
      path: file["filename"]
    }

    request_body = Map.merge(body, anchor(draft))

    case request.("/comments", method: :post, json: request_body) do
      {:ok, %{status: 422}} ->
        {422,
         %{
           error: "GitHub rejected the anchor. Your draft is still saved.",
           fileLevel: !draft.file_level && draft.replacement == :absent
         }}

      {:ok, %{status: status, body: body}} when status in 200..299 ->
        case Jason.decode(body) do
          {:ok, comment} -> {201, %{comment: comment}}
          _ -> uncertain_write()
        end

      {:ok, %{status: status}} ->
        access_error(status)

      {:error, _} ->
        uncertain_write()
    end
  end

  defp anchor(%{file_level: true}), do: %{subject_type: "file"}

  defp anchor(draft) do
    side = if draft.before, do: "LEFT", else: "RIGHT"
    anchor = %{line: draft.last, side: side}

    if draft.first == draft.last,
      do: anchor,
      else: Map.merge(anchor, %{start_line: draft.first, start_side: side})
  end

  defp comment_body(draft) do
    [
      draft.text,
      suggestion(draft.replacement),
      "<!-- repo-city:v1:#{draft.id}:#{draft.preset} -->"
    ]
    |> Enum.reject(&(&1 == ""))
    |> Enum.join("\n\n")
  end

  defp suggestion(:absent), do: ""

  defp suggestion(replacement) do
    length =
      ~r/`+/
      |> Regex.scan(replacement)
      |> Enum.reduce(3, fn [run], length -> max(length, byte_size(run) + 1) end)

    fence = String.duplicate("`", length)
    ending = if String.ends_with?(replacement, "\n"), do: "", else: "\n"
    fence <> "suggestion\n" <> replacement <> ending <> fence
  end

  defp next?(response),
    do:
      Enum.any?(Req.Response.get_header(response, "link"), &String.contains?(&1, ~s(rel="next")))

  defp matches?(text, pattern), do: is_binary(text) && Regex.match?(pattern, text)

  defp access_error(401), do: error(401, "Sign in with GitHub again to publish.")

  defp access_error(status) when status in [403, 404],
    do:
      error(
        403,
        "Publishing needs access to this repository and Pull requests: write on the Repo City GitHub App."
      )

  defp access_error(429), do: error(429, "GitHub rate limit reached. Your draft is still saved.")

  defp access_error(_),
    do: error(502, "GitHub did not confirm the request. Your draft is still saved.")

  defp uncertain_write,
    do: error(502, "GitHub did not confirm the write. Retry to check for the existing comment.")

  defp error(status, message), do: {status, %{error: message}}
end
