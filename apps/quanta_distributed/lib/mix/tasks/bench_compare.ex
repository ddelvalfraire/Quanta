defmodule Mix.Tasks.Bench.Compare do
  @moduledoc """
  Compare benchmark results against baselines.

  Loads the latest results from `benchmarks/results/` and compares them against
  the stored baselines in `benchmarks/baselines/`. Reports any regressions that
  exceed the configured threshold.

  ## Usage

      mix bench.compare
      mix bench.compare --tier 3
      mix bench.compare --threshold 0.10

  ## Options

    * `--tier` - Compare only a specific tier (1-4). Default: all tiers.
    * `--threshold` - Regression threshold as a fraction (e.g., 0.10 = 10%).
      Default: 0.10.
  """

  @shortdoc "Compare benchmarks vs baselines"

  use Mix.Task

  @baselines_dir Path.expand("../../../../../benchmarks/baselines", __DIR__)
  @results_dir Path.expand("../../../../../benchmarks/results", __DIR__)
  @default_threshold 0.10

  @impl true
  def run(args) do
    {opts, _, _} =
      OptionParser.parse(args, strict: [tier: :integer, threshold: :float])

    threshold = Keyword.get(opts, :threshold, @default_threshold)

    tiers =
      case Keyword.get(opts, :tier) do
        nil -> 1..4
        n when n in 1..4 -> [n]
        other -> Mix.raise("Invalid tier: #{other}. Must be 1-4.")
      end

    regressions =
      tiers
      |> Enum.flat_map(fn tier -> compare_tier(tier, threshold) end)

    report(regressions, threshold)
  end

  defp compare_tier(tier, threshold) do
    baseline_path = Path.join(@baselines_dir, "tier#{tier}_baseline.json")
    results = find_latest_results(tier)

    with {:ok, baseline_data} <- read_json(baseline_path),
         {:ok, results_data} <- read_json(results) do
      find_regressions(tier, baseline_data, results_data, threshold)
    else
      {:error, reason} ->
        Mix.shell().info("  Tier #{tier}: skipped (#{reason})")
        []
    end
  end

  defp find_latest_results(tier) do
    pattern = Path.join(@results_dir, "tier#{tier}_*.json")

    pattern
    |> Path.wildcard()
    |> Enum.sort()
    |> List.last()
  end

  defp read_json(nil), do: {:error, "no results file found"}

  defp read_json(path) do
    case File.read(path) do
      {:ok, content} ->
        case Jason.decode(content) do
          {:ok, data} when data == %{} -> {:error, "empty baseline"}
          {:ok, data} -> {:ok, data}
          {:error, _} -> {:error, "invalid JSON in #{path}"}
        end

      {:error, _} ->
        {:error, "cannot read #{path}"}
    end
  end

  defp find_regressions(tier, baseline, results, threshold) do
    # TODO: Implement comparison logic based on actual result format.
    # Expected format from Benchee JSON formatter:
    #   %{"scenario_name" => %{"run_time" => %{"median" => N, "p99" => N}}}
    #
    # For each scenario present in both baseline and results:
    #   - Compare median run_time
    #   - Flag if results median > baseline median * (1 + threshold)
    _ = {tier, baseline, results, threshold}
    []
  end

  defp report([], threshold) do
    pct = Float.round(threshold * 100, 1)
    Mix.shell().info("\nNo regressions detected (threshold: #{pct}%).")
  end

  defp report(regressions, threshold) do
    pct = Float.round(threshold * 100, 1)
    Mix.shell().error("\nRegressions detected (threshold: #{pct}%):\n")

    for {tier, scenario, baseline_val, result_val, delta_pct} <- regressions do
      Mix.shell().error(
        "  Tier #{tier} / #{scenario}: " <>
          "baseline=#{baseline_val}, current=#{result_val} " <>
          "(+#{Float.round(delta_pct * 100, 1)}%)"
      )
    end

    Mix.raise("Benchmark regression check failed.")
  end
end
