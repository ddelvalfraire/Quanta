defmodule Quanta.Wasm.EngineManager do
  @moduledoc """
  Holds Engine + Linker ResourceArcs for the WASM runtime.

  Stub — filled in by T06.
  """

  use GenServer

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts), do: {:ok, %{}}
end
