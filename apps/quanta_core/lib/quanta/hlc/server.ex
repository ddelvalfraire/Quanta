defmodule Quanta.HLC.Server do
  @moduledoc """
  GenServer wrapping the pure `Quanta.HLC` module.

  Maintains monotonic HLC state using the Kulkarni et al. algorithm:
  three-way merge of local wall, remote wall, and physical time.
  """

  use GenServer

  alias Quanta.HLC

  @max_logical 65_535

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @spec now() :: HLC.t()
  def now do
    GenServer.call(__MODULE__, :now)
  end

  @spec receive_event(HLC.t()) :: HLC.t()
  def receive_event(%HLC{} = remote) do
    GenServer.call(__MODULE__, {:receive_event, remote})
  end

  @impl true
  def init(_opts) do
    {:ok, %HLC{wall: System.os_time(:microsecond), logical: 0}}
  end

  @impl true
  def handle_call(:now, _from, state) do
    pt = System.os_time(:microsecond)
    wall = max(state.wall, pt)

    logical =
      if wall == state.wall do
        state.logical + 1
      else
        0
      end

    if logical > @max_logical do
      raise "HLC logical counter overflow (>#{@max_logical}): clock skew too large"
    end

    new_state = %HLC{wall: wall, logical: logical}
    {:reply, new_state, new_state}
  end

  @impl true
  def handle_call({:receive_event, remote}, _from, state) do
    pt = System.os_time(:microsecond)
    wall = Enum.max([state.wall, remote.wall, pt])

    logical =
      cond do
        wall == state.wall and wall == remote.wall ->
          max(state.logical, remote.logical) + 1

        wall == state.wall ->
          state.logical + 1

        wall == remote.wall ->
          remote.logical + 1

        true ->
          0
      end

    if logical > @max_logical do
      raise "HLC logical counter overflow (>#{@max_logical}): clock skew too large"
    end

    new_state = %HLC{wall: wall, logical: logical}
    {:reply, new_state, new_state}
  end
end
