defmodule Quanta.Actor.RegistryApiSurfaceTest do
  @moduledoc """
  Static-analysis guard against reaching into syn's private ETS layer.

  `:syn_backbone.get_table_name/2` is an undocumented helper that hands out the
  raw ETS table used by `syn` internals. Any call into it silently couples us
  to syn's private storage layout and will break on the next version upgrade
  (the table name convention is not part of syn's public API).

  This test walks every `.ex` file under `apps/*/lib/` and asserts that no
  non-comment line contains the `:syn_backbone` atom. Tests, docs, and
  comments are allowed to reference the name (so we can explain *why* we
  avoid it), but production source code must not.
  """

  use ExUnit.Case, async: true

  @banned_token ":syn_backbone"

  test "no production source file references :syn_backbone" do
    offenders =
      umbrella_root()
      |> Path.join("apps/*/lib/**/*.ex")
      |> Path.wildcard()
      |> Enum.flat_map(&scan_file/1)

    assert offenders == [],
           "Production code must not call the undocumented `:syn_backbone` API. " <>
             "Offenders:\n" <>
             Enum.map_join(offenders, "\n", fn {path, line_no, line} ->
               "  #{path}:#{line_no}: #{String.trim(line)}"
             end)
  end

  defp scan_file(path) do
    path
    |> File.read!()
    |> String.split("\n")
    |> Enum.with_index(1)
    |> Enum.filter(fn {line, _idx} -> production_line?(line) end)
    |> Enum.filter(fn {line, _idx} -> String.contains?(line, @banned_token) end)
    |> Enum.map(fn {line, idx} -> {path, idx, line} end)
  end

  # Skip lines that are purely comments — docstrings/comments may legitimately
  # reference `:syn_backbone` to explain why we moved away from it.
  defp production_line?(line) do
    trimmed = String.trim_leading(line)
    not String.starts_with?(trimmed, "#")
  end

  # Walk upward from the current app directory until we find the umbrella root
  # (the one that declares `apps_path: "apps"`). Works whether the test is run
  # from the umbrella root or from inside a specific app.
  defp umbrella_root do
    File.cwd!()
    |> Stream.iterate(&Path.dirname/1)
    |> Stream.take_while(&(&1 != "/"))
    |> Enum.find(File.cwd!(), fn dir -> File.dir?(Path.join(dir, "apps")) end)
  end
end
