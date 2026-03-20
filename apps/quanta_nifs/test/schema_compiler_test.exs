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
