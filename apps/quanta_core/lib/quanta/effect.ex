defmodule Quanta.Effect do
  @moduledoc """
  Tagged-tuple types representing actor side effects.
  """

  @type crdt_op ::
          {:text_insert, String.t(), non_neg_integer(), String.t()}
          | {:text_delete, String.t(), non_neg_integer(), non_neg_integer()}
          | {:text_mark, String.t(), non_neg_integer(), non_neg_integer(), String.t(), term()}
          | {:map_set, String.t(), String.t(), term()}
          | {:map_delete, String.t(), String.t()}
          | {:list_insert, String.t(), non_neg_integer(), term()}
          | {:list_delete, String.t(), non_neg_integer(), non_neg_integer()}
          | {:tree_move, String.t(), String.t(), String.t() | nil}

  @type t ::
          {:reply, binary()}
          | {:send, Quanta.ActorId.t(), binary()}
          | {:publish, String.t(), binary()}
          | {:persist, binary()}
          | {:set_timer, String.t(), pos_integer()}
          | {:cancel_timer, String.t()}
          | {:emit_telemetry, String.t(), map(), map()}
          | {:spawn_actor, Quanta.ActorId.t(), binary()}
          | :stop_self
          | {:side_effect, {module(), atom(), [term()]}}
          | {:crdt_ops, [crdt_op()]}
end
