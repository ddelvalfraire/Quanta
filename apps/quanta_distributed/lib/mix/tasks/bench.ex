defmodule Mix.Tasks.Bench do
  @moduledoc "Run all benchmark tiers."
  @shortdoc "Run all Quanta benchmarks"

  use Mix.Task

  @impl true
  def run(_args) do
    Mix.Task.run("app.start")

    for tier <- ~w(bench.tier1 bench.tier2 bench.tier3 bench.tier4 bench.tier5) do
      Mix.Task.run(tier)
    end
  end
end

defmodule Mix.Tasks.Bench.Tier1 do
  @moduledoc "Run Tier 1 (core actor runtime) benchmarks."
  @shortdoc "Run Tier 1 benchmarks"

  use Mix.Task

  @impl true
  def run(_args) do
    Mix.Task.run("app.start")
    IO.puts("\n╔══════════════════════════════════╗")
    IO.puts("║   Tier 1: Core Actor Runtime     ║")
    IO.puts("╚══════════════════════════════════╝\n")
    Quanta.Bench.Tier1.run_all()
  end
end

defmodule Mix.Tasks.Bench.Tier2 do
  @moduledoc "Run Tier 2 (virtual actor lifecycle) benchmarks."
  @shortdoc "Run Tier 2 benchmarks"

  use Mix.Task

  @impl true
  def run(_args) do
    Mix.Task.run("app.start")
    IO.puts("\n╔══════════════════════════════════╗")
    IO.puts("║  Tier 2: Virtual Actor Lifecycle  ║")
    IO.puts("╚══════════════════════════════════╝\n")
    Quanta.Bench.Tier2.run_all()
  end
end

defmodule Mix.Tasks.Bench.Tier3 do
  @moduledoc "Run Tier 3 (CRDT performance) benchmarks."
  @shortdoc "Run Tier 3 benchmarks"

  use Mix.Task

  @impl true
  def run(_args) do
    Mix.Task.run("app.start")
    IO.puts("\n╔══════════════════════════════════╗")
    IO.puts("║    Tier 3: CRDT Performance      ║")
    IO.puts("╚══════════════════════════════════╝\n")
    Quanta.Bench.Tier3.run_all()
  end
end

defmodule Mix.Tasks.Bench.Tier4 do
  @moduledoc "Run Tier 4 (differentiators) benchmarks."
  @shortdoc "Run Tier 4 benchmarks"

  use Mix.Task

  @impl true
  def run(_args) do
    Mix.Task.run("app.start")
    IO.puts("\n╔══════════════════════════════════╗")
    IO.puts("║     Tier 4: Differentiators      ║")
    IO.puts("╚══════════════════════════════════╝\n")
    Quanta.Bench.Tier4.run_all()
  end
end

defmodule Mix.Tasks.Bench.Tier5 do
  @moduledoc "Run Tier 5 (NIF delta encoding) benchmarks."
  @shortdoc "Run Tier 5 benchmarks"

  use Mix.Task

  @impl true
  def run(_args) do
    Mix.Task.run("app.start")
    IO.puts("\n╔══════════════════════════════════╗")
    IO.puts("║   Tier 5: NIF Delta Encoding     ║")
    IO.puts("╚══════════════════════════════════╝\n")
    Quanta.Bench.Tier5.run_all()
  end
end
