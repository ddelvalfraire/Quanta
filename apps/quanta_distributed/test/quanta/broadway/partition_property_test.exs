defmodule Quanta.Broadway.PartitionPropertyTest do
  @moduledoc """
  Property test P4: Broadway partition determinism.

  The partition function must be deterministic — the same subject always
  maps to the same partition. This guarantees per-actor ordering.
  """

  use ExUnit.Case, async: true
  use PropCheck

  alias Quanta.Broadway.EventProcessor

  @moduletag :property

  # ── Generators ──────────────────────────────────────────────────────

  @segment_chars ~c"abcdefghijklmnopqrstuvwxyz0123456789_-"

  defp segment_gen do
    let chars <- non_empty(list(oneof(@segment_chars))) do
      List.to_string(chars)
    end
  end

  defp actor_subject_gen do
    let {ns, type, actor_id} <- {segment_gen(), segment_gen(), segment_gen()} do
      "quanta.#{ns}.evt.#{type}.#{actor_id}"
    end
  end

  defp arbitrary_subject_gen do
    let parts <- non_empty(list(segment_gen())) do
      Enum.join(parts, ".")
    end
  end

  defp broadway_message(subject) do
    %Broadway.Message{
      data: "",
      acknowledger: {Broadway.NoopAcknowledger, nil, nil},
      metadata: %{subject: subject}
    }
  end

  # ── Properties ──────────────────────────────────────────────────────

  property "deterministic: same subject always maps to same partition" do
    forall subject <- actor_subject_gen() do
      msg = broadway_message(subject)
      p1 = EventProcessor.partition_by_actor_id(msg)
      p2 = EventProcessor.partition_by_actor_id(msg)
      p1 == p2
    end
  end

  property "same actor_id in different events maps to same partition" do
    forall {ns, type1, type2, actor_id} <-
             {segment_gen(), segment_gen(), segment_gen(), segment_gen()} do
      subject1 = "quanta.#{ns}.evt.#{type1}.#{actor_id}"
      subject2 = "quanta.#{ns}.evt.#{type2}.#{actor_id}"
      msg1 = broadway_message(subject1)
      msg2 = broadway_message(subject2)

      # Same actor_id should hash to same partition regardless of event type
      EventProcessor.partition_by_actor_id(msg1) ==
        EventProcessor.partition_by_actor_id(msg2)
    end
  end

  property "partition returns non-negative integer" do
    forall subject <- arbitrary_subject_gen() do
      msg = broadway_message(subject)
      result = EventProcessor.partition_by_actor_id(msg)
      is_integer(result) and result >= 0
    end
  end

  property "different actor_ids produce some distribution" do
    # Not a per-case property — aggregate check over many IDs
    forall ids <- vector(100, segment_gen()) do
      partitions =
        ids
        |> Enum.map(fn id ->
          msg = broadway_message("quanta.test.evt.cmd.#{id}")
          EventProcessor.partition_by_actor_id(msg)
        end)
        |> Enum.uniq()

      # With 100 random IDs we should see at least 2 distinct partitions
      length(partitions) >= 2
    end
  end
end
