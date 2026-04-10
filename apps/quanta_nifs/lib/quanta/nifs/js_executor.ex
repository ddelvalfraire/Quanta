defmodule Quanta.Nifs.JsExecutor do
  @moduledoc """
  Elixir wrapper for the JS execution NIF.

  Executes JavaScript source code via the Javy QuickJS WASM provider,
  capturing stdout and stderr output.
  """

  alias Quanta.Nifs.Native

  @default_fuel 1_000_000
  @default_memory 16_777_216

  @spec execute(binary(), binary(), pos_integer(), pos_integer()) ::
          {:ok, String.t(), String.t()} | {:error, atom() | String.t()}
  def execute(wasm_bytes, js_source, fuel \\ @default_fuel, memory_limit \\ @default_memory)
      when is_binary(wasm_bytes) and is_binary(js_source) do
    Native.execute_js(wasm_bytes, js_source, fuel, memory_limit)
  end
end
