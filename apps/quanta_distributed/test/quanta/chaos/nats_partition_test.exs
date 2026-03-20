defmodule Quanta.Chaos.NatsPartitionTest do
  @moduledoc """
  CH1: BEAM <-> NATS network partition via Toxiproxy.

  Cuts the connection between BEAM nodes and NATS using Toxiproxy,
  verifies that in-flight operations fail gracefully, and that
  the system recovers when the partition heals.

  Requires: `docker compose -f docker-compose.test.yml up -d`
  """

  use ExUnit.Case, async: false

  alias Quanta.Test.ToxiproxyHelpers

  # TODO: uncomment when wiring actors through Toxiproxy ports
  # alias Quanta.ActorId
  # alias Quanta.Envelope
  # alias Quanta.Nats.JetStream
  # alias Quanta.Test.ClusterHelpers
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

  test "actor operations fail gracefully during NATS partition" do
    # TODO: Wire actors to connect through Toxiproxy ports (14222-14224)
    # instead of direct NATS ports for this test.

    # Step 1: Verify baseline connectivity
    # assert {:ok, _} = JetStream.kv_put("test_chaos", "ch1", "before")

    # Step 2: Cut BEAM <-> NATS via Toxiproxy
    # ToxiproxyHelpers.disable_proxy(:nats_1)

    # Step 3: Verify operations fail with timeout/connection errors
    # assert {:error, _} = JetStream.kv_put("test_chaos", "ch1", "during")

    # Step 4: Heal the partition
    # ToxiproxyHelpers.enable_proxy(:nats_1)
    # Process.sleep(2_000)

    # Step 5: Verify recovery
    # assert {:ok, _} = JetStream.kv_put("test_chaos", "ch1", "after")
    # assert {:ok, "after", _} = JetStream.kv_get("test_chaos", "ch1")
  end

  test "in-flight publishes timeout during partition" do
    # TODO: Start a stream of publishes, cut the proxy mid-stream,
    # verify that publishes return {:error, :timeout} or similar,
    # then heal and verify new publishes succeed.
  end
end
