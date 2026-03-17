defmodule Quanta.Wasm.ModuleRegistry do
  @moduledoc """
  Maps `{namespace, type}` to compiled WASM ComponentResource.

  Stub — filled in by T06.
  """

  use GenServer

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts), do: {:ok, %{}}
end
