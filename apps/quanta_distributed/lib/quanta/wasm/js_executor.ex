defmodule Quanta.Wasm.JsExecutor do
  @moduledoc """
  GenServer that caches the Javy QuickJS WASM provider bytes and
  exposes a simple `execute/1` API for running JavaScript code.
  """

  use GenServer

  require Logger

  @default_fuel 1_000_000
  @default_memory 16_777_216

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @spec execute(String.t()) :: {:ok, String.t(), String.t()} | {:error, atom() | String.t()}
  def execute(js_source) when is_binary(js_source) do
    GenServer.call(__MODULE__, {:execute, js_source}, 30_000)
  end

  @impl true
  def init(_opts) do
    path = Application.app_dir(:quanta_nifs, "priv/wasm/javy_quickjs_provider.wasm")

    case File.read(path) do
      {:ok, wasm_bytes} ->
        Logger.info("[JsExecutor] Loaded Javy provider (#{byte_size(wasm_bytes)} bytes)")
        {:ok, %{wasm_bytes: wasm_bytes}}

      {:error, reason} ->
        Logger.warning("[JsExecutor] Failed to load Javy provider at #{path}: #{inspect(reason)}")
        {:ok, %{wasm_bytes: nil}}
    end
  end

  @impl true
  def handle_call({:execute, _js_source}, _from, %{wasm_bytes: nil} = state) do
    {:reply, {:error, "js_executor_not_available"}, state}
  end

  def handle_call({:execute, js_source}, _from, %{wasm_bytes: wasm_bytes} = state) do
    result = Quanta.Nifs.JsExecutor.execute(wasm_bytes, js_source, @default_fuel, @default_memory)
    {:reply, result, state}
  end
end
