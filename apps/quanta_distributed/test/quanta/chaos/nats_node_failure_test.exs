defmodule Quanta.Chaos.NatsNodeFailureTest do
  @moduledoc """
  CH3: Kill 1 of 3 NATS nodes in the cluster.

  Takes down a single NATS node via Toxiproxy and verifies that
  JetStream operations continue to work through the surviving nodes.

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  alias Quanta.Test.ToxiproxyHelpers

  # TODO: uncomment when wiring test NATS connection through Toxiproxy
  # alias Quanta.Nats.JetStream
  # alias Quanta.Test.NatsHelpers

  @moduletag :chaos
  @moduletag timeout: 180_000

  setup_all do
    :inets.start()
    :ssl.start()
    ToxiproxyHelpers.setup_proxies()
    on_exit(fn -> ToxiproxyHelpers.reset_all() end)
    :ok
  end

  setup do
    ToxiproxyHelpers.reset_all()
    :ok
  end

  test "JetStream survives loss of 1 NATS node" do
    # TODO: Wire the test NATS connection to use Toxiproxy ports.
    # The test plan:
    #
    # Step 1: Verify baseline — publish and read from KV
    # bucket = "test_nats_failure_#{:erlang.unique_integer([:positive])}"
    # NatsHelpers.ensure_kv_bucket(bucket)
    # assert {:ok, _} = JetStream.kv_put(bucket, "key1", "before")
    #
    # Step 2: Kill NATS node 2 via Toxiproxy
    # ToxiproxyHelpers.disable_proxy(:nats_2)
    # Process.sleep(2_000)
    #
    # Step 3: Operations should still work through surviving nodes
    # assert {:ok, _} = JetStream.kv_put(bucket, "key2", "during_failure")
    # assert {:ok, "before", _} = JetStream.kv_get(bucket, "key1")
    #
    # Step 4: Restore the node
    # ToxiproxyHelpers.enable_proxy(:nats_2)
    # Process.sleep(3_000)
    #
    # Step 5: All data should be consistent after rejoin
    # assert {:ok, "during_failure", _} = JetStream.kv_get(bucket, "key2")
    #
    # NatsHelpers.delete_kv_bucket(bucket)
  end

  test "KV writes during NATS node failure preserve data" do
    # TODO: Same as above but focused on KV consistency:
    # - Write N keys before failure
    # - Kill one NATS node
    # - Write N more keys
    # - Restore node
    # - Verify all 2N keys are present and correct
  end
end
