defmodule Quanta.Nifs.DeltaEncoderTest do
  use ExUnit.Case

  alias Quanta.Nifs.DeltaEncoder
  alias Quanta.Nifs.SchemaCompiler

  @mixed_types_wit """
  record game-state {
      is-alive: bool,
      /// @quanta:clamp(0, 100)
      health: u16,
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      pos-x: f32,
      score: s32,
  }
  """

  defp compile_schema(wit \\ @mixed_types_wit, type_name \\ "game-state") do
    {:ok, ref, _warnings} = SchemaCompiler.compile(wit, type_name)
    ref
  end

  describe "encode_state/2 and decode_state/2" do
    test "roundtrip with mixed types" do
      schema = compile_schema()

      values = [true, 75, 42.5, -10]
      assert {:ok, state} = DeltaEncoder.encode_state(schema, values)
      assert is_binary(state)

      assert {:ok, decoded} = DeltaEncoder.decode_state(schema, state)
      assert is_map(decoded)

      assert decoded["is-alive"] == true
      assert decoded["health"] == 75
      assert_in_delta decoded["pos-x"], 42.5, 0.01
      assert decoded["score"] == -10
    end

    test "bool false decodes correctly" do
      schema = compile_schema()

      values = [false, 0, 0.0, 0]
      assert {:ok, state} = DeltaEncoder.encode_state(schema, values)
      assert {:ok, decoded} = DeltaEncoder.decode_state(schema, state)

      assert decoded["is-alive"] == false
      assert decoded["health"] == 0
    end

    test "field count mismatch returns error" do
      schema = compile_schema()
      assert {:error, msg} = DeltaEncoder.encode_state(schema, [true, 50])
      assert msg =~ "field count mismatch"
    end
  end

  describe "compute_delta/4 and apply_delta/3" do
    test "full roundtrip: encode -> compute -> apply -> decode" do
      schema = compile_schema()

      old_values = [true, 50, 100.0, 0]
      new_values = [true, 75, -200.5, 42]

      assert {:ok, old_state} = DeltaEncoder.encode_state(schema, old_values)
      assert {:ok, new_state} = DeltaEncoder.encode_state(schema, new_values)

      assert {:ok, delta} = DeltaEncoder.compute_delta(schema, old_state, new_state)
      assert is_binary(delta)

      assert {:ok, result} = DeltaEncoder.apply_delta(schema, old_state, delta)
      assert result == new_state

      assert {:ok, decoded} = DeltaEncoder.decode_state(schema, result)
      assert decoded["health"] == 75
      assert_in_delta decoded["pos-x"], -200.5, 0.01
      assert decoded["score"] == 42
    end

    test "identity delta returns empty binary" do
      schema = compile_schema()

      values = [true, 50, 100.0, 10]
      assert {:ok, state} = DeltaEncoder.encode_state(schema, values)

      assert {:ok, delta} = DeltaEncoder.compute_delta(schema, state, state)
      assert delta == <<>>
    end

    test "applying empty delta returns current state" do
      schema = compile_schema()

      values = [true, 50, 100.0, 10]
      assert {:ok, state} = DeltaEncoder.encode_state(schema, values)

      assert {:ok, result} = DeltaEncoder.apply_delta(schema, state, <<>>)
      assert result == state
    end
  end

  describe "schema version mismatch" do
    test "apply_delta with tampered version byte returns :schema_version_mismatch" do
      schema = compile_schema()

      {:ok, old_state} = DeltaEncoder.encode_state(schema, [true, 50, 0.0, 0])
      {:ok, new_state} = DeltaEncoder.encode_state(schema, [false, 50, 0.0, 0])
      {:ok, delta} = DeltaEncoder.compute_delta(schema, old_state, new_state)

      <<flags, _ver, rest::binary>> = delta
      bad_delta = <<flags, 99, rest::binary>>

      assert {:error, :schema_version_mismatch} =
               DeltaEncoder.apply_delta(schema, old_state, bad_delta)
    end
  end

  describe "multiple field types" do
    test "bool, unsigned int, quantized float all work together" do
      schema = compile_schema()

      {:ok, state1} = DeltaEncoder.encode_state(schema, [true, 100, 5000.0, -100])
      {:ok, decoded1} = DeltaEncoder.decode_state(schema, state1)
      assert decoded1["is-alive"] == true
      assert decoded1["health"] == 100
      assert_in_delta decoded1["pos-x"], 5000.0, 0.01
      assert decoded1["score"] == -100

      {:ok, state2} = DeltaEncoder.encode_state(schema, [false, 100, 5000.0, -100])
      {:ok, delta} = DeltaEncoder.compute_delta(schema, state1, state2)
      assert byte_size(delta) > 0

      {:ok, result} = DeltaEncoder.apply_delta(schema, state1, delta)
      {:ok, decoded_result} = DeltaEncoder.decode_state(schema, result)
      assert decoded_result["is-alive"] == false
      assert decoded_result["health"] == 100
    end
  end

  describe "quantize_state/2" do
    @quantize_wit """
    record player-state {
        /// @quanta:quantize(0.1)
        /// @quanta:clamp(0, 100)
        health: f32,
        ammo: u16,
        /// @quanta:quantize(0.01)
        /// @quanta:clamp(-10000, 10000)
        pos-x: f32,
        is-alive: bool,
        score: s32,
    }
    """

    defp build_native_state(fields) do
      # Build native-packed bitstring: each field at its native IEEE width
      bits =
        for {type, value} <- fields, into: <<>> do
          case type do
            :f32 -> encode_f32(value)
            :u16 -> <<value::16>>
            :bool -> <<value::1>>
            :s32 -> <<value::signed-32>>
          end
        end

      # Pad to byte boundary (BitWriter in Rust does the same)
      total = bit_size(bits)
      pad_bits = rem(8 - rem(total, 8), 8)
      <<bits::bitstring, 0::size(pad_bits)>>
    end

    defp encode_f32(:nan), do: <<0x7FC00000::32>>
    defp encode_f32(:infinity), do: <<0x7F800000::32>>
    defp encode_f32(value) when is_number(value), do: <<value::float-32>>

    test "f32 field with quantize(0.1) + clamp(0, 100) quantizes correctly" do
      schema = compile_schema(@quantize_wit, "player-state")

      native = build_native_state([
        {:f32, 50.15},
        {:u16, 100},
        {:f32, 42.5},
        {:bool, 1},
        {:s32, -10}
      ])

      assert {:ok, quantized} = DeltaEncoder.quantize_state(schema, native)
      assert is_binary(quantized)

      assert {:ok, decoded} = DeltaEncoder.decode_state(schema, quantized)
      # 50.15 with precision 0.1: round(501.5) = 502 → 50.2 (round-half-away-from-zero)
      assert decoded["health"] == 50.2
      assert decoded["ammo"] == 100
      assert_in_delta decoded["pos-x"], 42.5, 0.01
      assert decoded["is-alive"] == true
      assert decoded["score"] == -10
    end

    test "NaN in quantized field returns error with field name" do
      schema = compile_schema(@quantize_wit, "player-state")

      # IEEE 754 f32 NaN: 0x7FC00000
      native = build_native_state([
        {:f32, :nan},
        {:u16, 0},
        {:f32, 0.0},
        {:bool, 0},
        {:s32, 0}
      ])

      assert {:error, msg} = DeltaEncoder.quantize_state(schema, native)
      assert msg =~ "NaN or infinity"
      assert msg =~ "health"
    end

    test "Infinity in quantized field returns error" do
      schema = compile_schema(@quantize_wit, "player-state")

      native = build_native_state([
        {:f32, :infinity},
        {:u16, 0},
        {:f32, 0.0},
        {:bool, 0},
        {:s32, 0}
      ])

      assert {:error, msg} = DeltaEncoder.quantize_state(schema, native)
      assert msg =~ "NaN or infinity"
    end

    test "delta comparison after quantize uses exact integer equality" do
      schema = compile_schema(@quantize_wit, "player-state")

      # Two native states with values that differ within precision
      native_a = build_native_state([
        {:f32, 50.101},
        {:u16, 100},
        {:f32, 42.501},
        {:bool, 1},
        {:s32, 0}
      ])

      native_b = build_native_state([
        {:f32, 50.104},  # within 0.1 precision of 50.101
        {:u16, 100},
        {:f32, 42.504},  # within 0.01 precision of 42.501
        {:bool, 1},
        {:s32, 0}
      ])

      assert {:ok, q_a} = DeltaEncoder.quantize_state(schema, native_a)
      assert {:ok, q_b} = DeltaEncoder.quantize_state(schema, native_b)

      # Both quantize to the same packed integers → empty delta
      assert {:ok, delta} = DeltaEncoder.compute_delta(schema, q_a, q_b)
      assert delta == <<>>
    end

    test "roundtrip: quantize → decode → values match expected" do
      schema = compile_schema(@quantize_wit, "player-state")

      native = build_native_state([
        {:f32, 75.3},
        {:u16, 500},
        {:f32, -3000.55},
        {:bool, 0},
        {:s32, 42}
      ])

      assert {:ok, quantized} = DeltaEncoder.quantize_state(schema, native)
      assert {:ok, decoded} = DeltaEncoder.decode_state(schema, quantized)

      assert_in_delta decoded["health"], 75.3, 0.1
      assert decoded["ammo"] == 500
      assert_in_delta decoded["pos-x"], -3000.55, 0.01
      assert decoded["is-alive"] == false
      assert decoded["score"] == 42
    end
  end
end
