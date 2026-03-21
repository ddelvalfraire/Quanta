defmodule Quanta.Bench.Tier5.NifDelta do
  @moduledoc "B5.1/B5.2 -- NIF delta encoding benchmarks: schema compile, compute/apply delta, decode state."

  alias Quanta.Bench.Base
  alias Quanta.Nifs.DeltaEncoder
  alias Quanta.Nifs.SchemaCompiler

  @wit_20f """
  record bench-state {
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      field-0: f32,
      /// @quanta:clamp(0, 65535)
      field-1: u16,
      field-2: bool,
      field-3: u32,
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      field-4: f32,
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      field-5: f32,
      /// @quanta:clamp(0, 65535)
      field-6: u16,
      field-7: bool,
      field-8: u32,
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      field-9: f32,
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      field-10: f32,
      /// @quanta:clamp(0, 65535)
      field-11: u16,
      field-12: bool,
      field-13: u32,
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      field-14: f32,
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      field-15: f32,
      /// @quanta:clamp(0, 65535)
      field-16: u16,
      field-17: bool,
      field-18: u32,
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      field-19: f32,
  }
  """

  @spec run :: :ok
  def run do
    run_sequential()
    run_concurrent()
  end

  defp run_sequential do
    schema = compile_schema()
    {old_state, new_10, new_50, new_100} = build_states(schema)

    {:ok, delta_50} = DeltaEncoder.compute_delta(schema, old_state, new_50)

    Base.run("tier5_nif_delta", %{
      "compile_schema_20f" => fn ->
        {:ok, _ref, _warnings} = SchemaCompiler.compile(@wit_20f, "bench-state")
      end,
      "compute_delta_20f_10pct" => fn ->
        {:ok, _delta} = DeltaEncoder.compute_delta(schema, old_state, new_10)
      end,
      "compute_delta_20f_50pct" => fn ->
        {:ok, _delta} = DeltaEncoder.compute_delta(schema, old_state, new_50)
      end,
      "compute_delta_20f_100pct" => fn ->
        {:ok, _delta} = DeltaEncoder.compute_delta(schema, old_state, new_100)
      end,
      "apply_delta_20f" => fn ->
        {:ok, _state} = DeltaEncoder.apply_delta(schema, old_state, delta_50)
      end,
      "decode_state_20f" => fn ->
        {:ok, _map} = DeltaEncoder.decode_state(schema, old_state)
      end
    }, memory_time: 2)
  end

  defp run_concurrent do
    schema = compile_schema()
    {old_state, _new_10, new_50, _new_100} = build_states(schema)

    for concurrency <- [1, 4, 8, 16] do
      Base.run("tier5_nif_delta_#{concurrency}p", %{
        "compute_delta_20f_50pct" => fn ->
          {:ok, _delta} = DeltaEncoder.compute_delta(schema, old_state, new_50)
        end
      }, parallel: concurrency, memory_time: 2)
    end

    :ok
  end

  defp compile_schema do
    {:ok, ref, _warnings} = SchemaCompiler.compile(@wit_20f, "bench-state")
    ref
  end

  defp build_states(schema) do
    base_values = List.duplicate(0, 20)
    {:ok, old_state} = DeltaEncoder.encode_state(schema, base_values)

    # 10% change: 2 fields differ
    new_10_values = [1, 1 | List.duplicate(0, 18)]
    {:ok, new_10} = DeltaEncoder.encode_state(schema, new_10_values)

    # 50% change: 10 fields differ
    new_50_values = List.duplicate(1, 10) ++ List.duplicate(0, 10)
    {:ok, new_50} = DeltaEncoder.encode_state(schema, new_50_values)

    # 100% change: all fields differ
    new_100_values = List.duplicate(1, 20)
    {:ok, new_100} = DeltaEncoder.encode_state(schema, new_100_values)

    {old_state, new_10, new_50, new_100}
  end
end
