defmodule Quanta.Nifs.SchemaCompiler do
  @moduledoc false

  alias Quanta.Nifs.Native

  @spec compile(wit_source :: String.t(), type_name :: String.t(), prediction_enabled :: boolean()) ::
          {:ok, reference(), [String.t()]} | {:error, String.t()}
  def compile(wit_source, type_name, prediction_enabled \\ false)
      when is_binary(wit_source) and is_binary(type_name) and is_boolean(prediction_enabled) do
    Native.schema_compile(wit_source, type_name, prediction_enabled)
  end

  @spec export(schema :: reference()) :: {:ok, binary()}
  def export(schema) do
    Native.schema_export(schema)
  end

  @spec import_schema(bytes :: binary()) :: {:ok, reference()} | {:error, String.t()}
  def import_schema(bytes) when is_binary(bytes) do
    Native.schema_import(bytes)
  end

  @spec check_compatibility(old :: reference(), new :: reference()) ::
          {:ok, :identical | :compatible | :incompatible, String.t()}
  def check_compatibility(old, new) do
    Native.schema_check_compatibility(old, new)
  end
end
