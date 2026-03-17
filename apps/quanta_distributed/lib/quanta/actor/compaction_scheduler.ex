defmodule Quanta.Actor.CompactionScheduler do
  @moduledoc """
  Deferred event compaction scheduler.

  Stub — filled in by T13.
  """

  use GenServer

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts), do: {:ok, %{}}
end
