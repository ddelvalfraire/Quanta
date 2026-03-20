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
      assert format_ver == 1
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
end
