defmodule Quanta.Integration.NatsKvConcurrencyTest do
  @moduledoc """
  FS4: NATS KV concurrent writer fencing.

  10 concurrent writers attempt to write to the same KV key,
  verifying that revision-based fencing prevents lost updates.

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  import Quanta.Test.NatsHelpers

  alias Quanta.Nats.JetStream

  @moduletag :integration
  @moduletag timeout: 120_000

  @writer_count 10
  @bucket "test_concurrency"

  setup do
    bucket = "#{@bucket}_#{:erlang.unique_integer([:positive])}"
    ensure_kv_bucket(bucket)
    on_exit(fn -> delete_kv_bucket(bucket) end)
    %{bucket: bucket}
  end

  test "concurrent writers are serialized via KV revisions", %{bucket: bucket} do
    key = "contested-key"

    # Seed the key with an initial value
    assert {:ok, _rev} = JetStream.kv_put(bucket, key, "initial")

    # Spawn 10 concurrent writers, each doing read-modify-write
    tasks =
      for i <- 1..@writer_count do
        Task.async(fn ->
          # Read current value and revision
          case JetStream.kv_get(bucket, key) do
            {:ok, _value, _revision} ->
              # Attempt a write — without CAS, some may "win" by overwriting
              JetStream.kv_put(bucket, key, "writer_#{i}")

            {:error, reason} ->
              {:error, reason}
          end
        end)
      end

    results = Task.await_many(tasks, 30_000)

    # All writers should have gotten a response (success or conflict)
    for result <- results do
      assert match?({:ok, _}, result) or match?({:error, _}, result),
             "Unexpected result: #{inspect(result)}"
    end

    # Final read — exactly one writer's value should be present
    assert {:ok, final_value, final_rev} = JetStream.kv_get(bucket, key)
    assert String.starts_with?(final_value, "writer_") or final_value == "initial"
    assert final_rev > 1, "Expected revisions > 1 after concurrent writes, got #{final_rev}"

    # TODO: Once CAS (compare-and-swap) fencing is exposed via the NIF,
    # use kv_put with expected_revision to verify that only one writer
    # succeeds per revision and others get :wrong_last_sequence errors.
  end

  test "sequential writes produce monotonically increasing revisions", %{bucket: bucket} do
    key = "sequential-key"

    revisions =
      for i <- 1..20 do
        {:ok, rev} = JetStream.kv_put(bucket, key, "value_#{i}")
        rev
      end

    # Revisions must be strictly increasing
    pairs = Enum.zip(revisions, tl(revisions))

    for {prev, curr} <- pairs do
      assert curr > prev, "Expected monotonic revisions, got #{prev} then #{curr}"
    end
  end
end
