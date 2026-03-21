defmodule Quanta.Nifs.SchemaCompilerTest do
  use ExUnit.Case

  alias Quanta.Nifs.SchemaCompiler

  @minimal_wit """
  record my-state {
      alive: bool,
  }
  """

  @quantized_wit """
  record player-state {
      /// @quanta:quantize(0.01)
      /// @quanta:clamp(-10000, 10000)
      pos-x: f32,
      is-alive: bool,
  }
  """

  @warn_wit """
  record my-state {
      /// @quanta:quantize(0.01)
      x: f32,
  }
  """

  @invalid_string_wit """
  record my-state {
      name: string,
  }
  """

  @predict_wit """
  record my-state {
      /// @quanta:predict(input_replay)
      x: f32,
  }
  """

  @predict_non_numeric_wit """
  record my-state {
      /// @quanta:predict(cosmetic)
      alive: bool,
  }
  """

  @smooth_lerp_non_numeric_wit """
  record my-state {
      /// @quanta:smooth(lerp, 100)
      alive: bool,
  }
  """

  @smooth_modes_wit """
  record my-state {
      /// @quanta:smooth(lerp, 100)
      x: f32,
      /// @quanta:smooth(snap)
      y: f32,
      /// @quanta:smooth(snap_lerp, 150, 5.0)
      z: f32,
  }
  """

  @predict_modes_wit """
  record my-state {
      /// @quanta:predict(none)
      a: f32,
      /// @quanta:predict(input_replay)
      b: f32,
      /// @quanta:predict(cosmetic)
      c: f32,
  }
  """

  @three_group_wit """
  record entity-state {
      /// @quanta:field_group(spatial)
      /// @quanta:priority(critical)
      pos-x: f32,
      /// @quanta:field_group(spatial)
      /// @quanta:priority(critical)
      pos-y: f32,
      /// @quanta:field_group(spatial)
      /// @quanta:priority(critical)
      pos-z: f32,
      /// @quanta:field_group(combat)
      /// @quanta:priority(high)
      health: u16,
      /// @quanta:field_group(combat)
      /// @quanta:priority(high)
      mana: u16,
      /// @quanta:field_group(combat)
      /// @quanta:priority(high)
      damage: u8,
      /// @quanta:field_group(combat)
      /// @quanta:priority(high)
      armor: u8,
      /// @quanta:field_group(inventory)
      /// @quanta:priority(low)
      slot1: u8,
      /// @quanta:field_group(inventory)
      /// @quanta:priority(low)
      slot2: u8,
      /// @quanta:field_group(inventory)
      /// @quanta:priority(low)
      slot3: u8,
  }
  """

  describe "compile/2" do
    test "returns reference and empty warnings for minimal schema" do
      assert {:ok, ref, warnings} = SchemaCompiler.compile(@minimal_wit, "my-state")
      assert is_reference(ref)
      assert warnings == []
    end

    test "returns error for missing type" do
      assert {:error, msg} = SchemaCompiler.compile(@minimal_wit, "nonexistent")
      assert is_binary(msg)
      assert msg =~ "not found"
    end

    test "returns error for string without skip_delta" do
      assert {:error, msg} = SchemaCompiler.compile(@invalid_string_wit, "my-state")
      assert is_binary(msg)
      assert msg =~ "skip_delta"
    end

    test "returns warnings for quantize without clamp" do
      assert {:ok, _ref, warnings} = SchemaCompiler.compile(@warn_wit, "my-state")
      assert length(warnings) > 0
      assert Enum.any?(warnings, &String.contains?(&1, "quantize without clamp"))
    end

    test "compiles quantized schema successfully" do
      assert {:ok, ref, warnings} = SchemaCompiler.compile(@quantized_wit, "player-state")
      assert is_reference(ref)
      assert warnings == []
    end

    test "predict(input_replay) without prediction_enabled returns error" do
      assert {:error, msg} = SchemaCompiler.compile(@predict_wit, "my-state", false)
      assert msg =~ "prediction enabled"
    end

    test "predict(input_replay) with prediction_enabled succeeds" do
      assert {:ok, _ref, _warnings} = SchemaCompiler.compile(@predict_wit, "my-state", true)
    end

    test "predict on non-numeric returns warning" do
      assert {:ok, _ref, warnings} =
               SchemaCompiler.compile(@predict_non_numeric_wit, "my-state", true)

      assert Enum.any?(warnings, &String.contains?(&1, "predict on non-numeric"))
    end

    test "smooth(lerp) on non-numeric returns error" do
      assert {:error, msg} = SchemaCompiler.compile(@smooth_lerp_non_numeric_wit, "my-state")
      assert msg =~ "non-numeric"
    end

    test "all 3 predict modes compile with prediction_enabled" do
      assert {:ok, _ref, warnings} =
               SchemaCompiler.compile(@predict_modes_wit, "my-state", true)

      assert warnings == []
    end

    test "all 3 smooth modes compile" do
      assert {:ok, _ref, warnings} = SchemaCompiler.compile(@smooth_modes_wit, "my-state")
      assert warnings == []
    end

    test "defaults: no annotation gives predict(none) and smooth(snap)" do
      assert {:ok, ref, _warnings} = SchemaCompiler.compile(@minimal_wit, "my-state")
      assert {:ok, binary} = SchemaCompiler.export(ref)
      # The binary should contain smoothing data (has_smoothing flag set)
      assert is_binary(binary)
      assert byte_size(binary) > 14
    end
  end

  describe "export/1" do
    test "returns binary with QSCH magic" do
      {:ok, ref, _} = SchemaCompiler.compile(@minimal_wit, "my-state")
      assert {:ok, binary} = SchemaCompiler.export(ref)
      assert is_binary(binary)
      assert <<"QSCH", _rest::binary>> = binary
    end

    test "export is deterministic" do
      {:ok, ref1, _} = SchemaCompiler.compile(@quantized_wit, "player-state")
      {:ok, ref2, _} = SchemaCompiler.compile(@quantized_wit, "player-state")
      assert {:ok, bytes1} = SchemaCompiler.export(ref1)
      assert {:ok, bytes2} = SchemaCompiler.export(ref2)
      assert bytes1 == bytes2
    end

    test "export contains correct format version" do
      {:ok, ref, _} = SchemaCompiler.compile(@minimal_wit, "my-state")
      {:ok, <<"QSCH", format_ver, _rest::binary>>} = SchemaCompiler.export(ref)
      assert format_ver == 2
    end

    test "export includes prediction metadata" do
      {:ok, ref, _} = SchemaCompiler.compile(@predict_wit, "my-state", true)
      assert {:ok, binary} = SchemaCompiler.export(ref)
      assert <<"QSCH", _rest::binary>> = binary
      assert byte_size(binary) > 14
    end

    test "export includes group count for multi-group schema" do
      {:ok, ref, _} = SchemaCompiler.compile(@three_group_wit, "entity-state")
      {:ok, binary} = SchemaCompiler.export(ref)

      <<"QSCH", _format_ver, _schema_ver, field_count::big-16, group_count, _rest::binary>> =
        binary

      assert field_count == 10
      assert group_count == 3
    end
  end

  describe "import_schema/1" do
    test "import/export roundtrip returns usable reference" do
      {:ok, ref, _} = SchemaCompiler.compile(@quantized_wit, "player-state")
      {:ok, bytes} = SchemaCompiler.export(ref)
      assert {:ok, imported} = SchemaCompiler.import_schema(bytes)
      assert is_reference(imported)

      # Re-export should produce identical bytes
      assert {:ok, ^bytes} = SchemaCompiler.export(imported)
    end

    test "returns error for invalid bytes" do
      assert {:error, msg} = SchemaCompiler.import_schema(<<"BAAD">>)
      assert is_binary(msg)
    end

    test "returns error for truncated data" do
      {:ok, ref, _} = SchemaCompiler.compile(@minimal_wit, "my-state")
      {:ok, bytes} = SchemaCompiler.export(ref)
      truncated = binary_part(bytes, 0, 6)
      assert {:error, _msg} = SchemaCompiler.import_schema(truncated)
    end
  end

  describe "check_compatibility/2" do
    test "identical schemas return :identical" do
      {:ok, ref1, _} = SchemaCompiler.compile(@minimal_wit, "my-state")
      {:ok, ref2, _} = SchemaCompiler.compile(@minimal_wit, "my-state")
      assert {:ok, :identical, _} = SchemaCompiler.check_compatibility(ref1, ref2)
    end

    test "appended field returns :compatible" do
      old_wit = """
      record my-state {
          alive: bool,
      }
      """

      new_wit = """
      record my-state {
          alive: bool,
          health: u16,
      }
      """

      {:ok, old_ref, _} = SchemaCompiler.compile(old_wit, "my-state")
      {:ok, new_ref, _} = SchemaCompiler.compile(new_wit, "my-state")
      assert {:ok, :compatible, details} = SchemaCompiler.check_compatibility(old_ref, new_ref)
      assert details =~ "health"
    end

    test "changed field type returns :incompatible" do
      old_wit = """
      record my-state {
          value: u16,
      }
      """

      new_wit = """
      record my-state {
          value: u32,
      }
      """

      {:ok, old_ref, _} = SchemaCompiler.compile(old_wit, "my-state")
      {:ok, new_ref, _} = SchemaCompiler.compile(new_wit, "my-state")
      assert {:ok, :incompatible, details} = SchemaCompiler.check_compatibility(old_ref, new_ref)
      assert details =~ "type changed"
    end

    test "import then check compatibility roundtrip" do
      {:ok, ref, _} = SchemaCompiler.compile(@quantized_wit, "player-state")
      {:ok, bytes} = SchemaCompiler.export(ref)
      {:ok, imported} = SchemaCompiler.import_schema(bytes)
      assert {:ok, :identical, _} = SchemaCompiler.check_compatibility(ref, imported)
    end
  end
end
