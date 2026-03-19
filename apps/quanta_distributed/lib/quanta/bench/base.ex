defmodule Quanta.Bench.Base do
  @moduledoc """
  Shared helpers for Quanta benchmarks.

  Provides actor setup, GC tracking, Benchee formatters, and result output.
  """

  @results_dir Path.expand("../../../../../benchmarks/results", __DIR__)

  @doc "Returns the absolute path to the benchmarks/results directory."
  @spec results_dir :: String.t()
  def results_dir, do: @results_dir

  @doc "Returns a timestamped results path for a given tier."
  @spec results_path(String.t()) :: String.t()
  def results_path(tier) do
    ts = DateTime.utc_now() |> DateTime.to_iso8601(:basic) |> String.replace(~r/[^0-9T]/, "")
    Path.join(@results_dir, "#{tier}_#{ts}.json")
  end

  @doc "Default Benchee formatters — console + JSON output."
  @spec formatters(String.t()) :: [term()]
  def formatters(tier) do
    File.mkdir_p!(@results_dir)
    path = results_path(tier)

    [
      Benchee.Formatters.Console,
      {Benchee.Formatters.JSON, file: path}
    ]
  end

  @doc "Forces a full GC on the calling process and returns reclaimed words."
  @spec gc_reclaim :: non_neg_integer()
  def gc_reclaim do
    before = :erlang.process_info(self(), :heap_size) |> elem(1)
    :erlang.garbage_collect()
    after_gc = :erlang.process_info(self(), :heap_size) |> elem(1)
    max(before - after_gc, 0)
  end

  @doc "Runs Benchee with standard config for a given tier."
  @spec run(String.t(), map(), keyword()) :: :ok
  def run(tier, scenarios, opts \\ []) do
    warmup = Keyword.get(opts, :warmup, 2)
    time = Keyword.get(opts, :time, 5)

    Benchee.run(scenarios,
      warmup: warmup,
      time: time,
      formatters: formatters(tier),
      print: [configuration: false]
    )

    :ok
  end
end
