defmodule Quanta.Actor.ServerEphemeralMatchErrorTest do
  use ExUnit.Case, async: true

  # MEDIUM-3: `Quanta.Nifs.EphemeralStore.encode/2` and siblings return
  # `{:ok, binary} | {:error, reason}`. A bare `{:ok, _} = ...` pattern match
  # inside a GenServer callback raises `MatchError` on `{:error, _}`, which
  # crashes the actor process. The NIF spec does not guarantee the error arm
  # is unreachable, so we must handle it explicitly (with `case` or `with`).
  #
  # This is a static lint test: it walks `server.ex` line-by-line and fails if
  # any bare `{:ok, _} = <EphemeralStore encode call>` pattern remains.

  @server_path Path.expand("../../../lib/quanta/actor/server.ex", __DIR__)

  @encode_call_fragments [
    "EphemeralStore.encode",
    "Quanta.Nifs.EphemeralStore.encode",
    ":ephemeral_store.encode"
  ]

  describe "server.ex" do
    test "file exists at expected path" do
      assert File.exists?(@server_path), "server.ex not found at #{@server_path}"
    end

    test "no bare {:ok, _} match on EphemeralStore.encode calls" do
      offenders =
        @server_path
        |> File.read!()
        |> String.split("\n")
        |> Enum.with_index(1)
        |> Enum.filter(&bare_ok_match_on_encode?/1)

      assert offenders == [],
             """
             Found bare `{:ok, _} = EphemeralStore.encode(...)` patterns in server.ex.
             These crash the actor with MatchError when the NIF returns {:error, _}.
             Replace with a `case` or `with` that handles the error arm explicitly.

             Offending lines:
             #{Enum.map_join(offenders, "\n", fn {line, n} -> "  #{n}: #{String.trim(line)}" end)}
             """
    end
  end

  defp bare_ok_match_on_encode?({line, _lineno}) do
    trimmed = String.trim(line)

    Enum.any?(@encode_call_fragments, fn fragment ->
      String.contains?(trimmed, fragment) and bare_ok_assignment?(trimmed)
    end)
  end

  # Matches `{:ok, <anything>} = ...` as a statement start (LHS of `=`).
  # Rejects `case ... do`, `with ...`, `when ...`, etc. by requiring the line
  # to begin with `{:ok`.
  defp bare_ok_assignment?(trimmed) do
    String.starts_with?(trimmed, "{:ok,") and String.contains?(trimmed, "} =")
  end
end
