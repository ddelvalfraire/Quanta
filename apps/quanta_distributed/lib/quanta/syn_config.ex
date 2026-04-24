defmodule Quanta.SynConfig do
  @moduledoc """
  Supervised worker that configures `:syn` for the node.

  Historically `QuantaDistributed.Application.start/2` performed
  `:syn.add_node_to_scopes/1` and `:syn.set_event_handler/1` BEFORE
  `Supervisor.start_link`. If either call raised — scope conflict on cluster
  rejoin, syn not yet started, etc. — the Application callback crashed and
  the node failed to boot.

  This worker moves the calls inside a supervised `init/1`, so the syn
  configuration is part of the supervision tree and benefits from normal
  restart semantics. The local actor index is initialised here too, before
  syn is told about our scopes, so the event handler's callbacks into the
  local mirror are guaranteed to find the table already there.

  Start this worker as the FIRST child of `Quanta.Supervisor` (via
  `rest_for_one`) so every subsequent child can assume syn is configured.
  """

  use GenServer

  alias Quanta.Actor.Registry

  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts) do
    # Must be initialised before syn, so the event handler's callbacks into
    # the local mirror are guaranteed to find the table already there.
    Registry.init_local_index()

    :ok = :syn.add_node_to_scopes(Registry.scopes())
    :ok = :syn.set_event_handler(Quanta.Actor.SynEventHandler)

    {:ok, %{}}
  end
end
