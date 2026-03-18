defmodule Quanta.Nifs.WasmRuntime do
  @moduledoc """
  Elixir wrapper for WASM component runtime NIFs.

  Provides the full actor lifecycle: engine creation, component compilation,
  serialization with HMAC, and actor entry-point calls (init, handle_message,
  handle_timer, migrate, on_passivate).
  """

  alias Quanta.Nifs.Native

  @doc "Create a new WASM engine with fuel metering and component model support."
  @spec engine_new() :: {:ok, reference()} | {:error, term()}
  def engine_new do
    Native.engine_new()
  end

  @doc "Compile a WASM component from binary bytes."
  @spec component_compile(reference(), binary()) :: {:ok, reference()} | {:error, term()}
  def component_compile(engine, wasm_bytes) when is_binary(wasm_bytes) do
    Native.component_compile(engine, wasm_bytes)
  end

  @doc "Create a new component linker bound to an engine."
  @spec linker_new(reference()) :: {:ok, reference()} | {:error, term()}
  def linker_new(engine) do
    Native.linker_new(engine)
  end

  @doc "Serialize a compiled component with HMAC-SHA256 integrity tag."
  @spec component_serialize(reference(), binary()) :: {:ok, binary()} | {:error, term()}
  def component_serialize(component, hmac_key) when is_binary(hmac_key) do
    Native.component_serialize(component, hmac_key)
  end

  @doc "Deserialize a component, verifying HMAC-SHA256 tag first."
  @spec component_deserialize(reference(), binary(), binary()) ::
          {:ok, reference()} | {:error, :hmac_mismatch} | {:error, term()}
  def component_deserialize(engine, bytes, hmac_key)
      when is_binary(bytes) and is_binary(hmac_key) do
    Native.component_deserialize(engine, bytes, hmac_key)
  end

  @doc "Call the actor's init function. Returns {:ok, state, effects} or {:error, reason}."
  @spec call_init(reference(), reference(), reference(), binary(), non_neg_integer(), non_neg_integer()) ::
          {:ok, binary(), [map()]} | {:error, term()}
  def call_init(engine, component, linker, payload, fuel, memory_limit)
      when is_binary(payload) do
    Native.call_init(engine, component, linker, payload, fuel, memory_limit)
  end

  @doc "Call the actor's handle-message function."
  @spec call_handle_message(reference(), reference(), reference(), binary(), map(), non_neg_integer(), non_neg_integer()) ::
          {:ok, binary(), [map()]} | {:error, term()}
  def call_handle_message(engine, component, linker, state, envelope, fuel, memory_limit)
      when is_binary(state) and is_map(envelope) do
    Native.call_handle_message(engine, component, linker, state, envelope, fuel, memory_limit)
  end

  @doc "Call the actor's handle-timer function."
  @spec call_handle_timer(reference(), reference(), reference(), binary(), String.t(), non_neg_integer(), non_neg_integer()) ::
          {:ok, binary(), [map()]} | {:error, term()}
  def call_handle_timer(engine, component, linker, state, timer_name, fuel, memory_limit)
      when is_binary(state) and is_binary(timer_name) do
    Native.call_handle_timer(engine, component, linker, state, timer_name, fuel, memory_limit)
  end

  @doc "Call the actor's migrate function. Returns {:error, :not_exported} if the export is absent."
  @spec call_migrate(reference(), reference(), reference(), binary(), non_neg_integer(), non_neg_integer(), non_neg_integer()) ::
          {:ok, binary(), [map()]} | {:error, :not_exported} | {:error, term()}
  def call_migrate(engine, component, linker, state, from_version, fuel, memory_limit)
      when is_binary(state) and is_integer(from_version) do
    Native.call_migrate(engine, component, linker, state, from_version, fuel, memory_limit)
  end

  @doc "Call the actor's on-passivate function. Returns {:error, :not_exported} if the export is absent."
  @spec call_on_passivate(reference(), reference(), reference(), binary(), non_neg_integer(), non_neg_integer()) ::
          {:ok, binary()} | {:error, :not_exported} | {:error, term()}
  def call_on_passivate(engine, component, linker, state, fuel, memory_limit)
      when is_binary(state) do
    Native.call_on_passivate(engine, component, linker, state, fuel, memory_limit)
  end
end
