defmodule Quanta.Broadway.NatsProducerTest do
  use ExUnit.Case, async: true

  alias Quanta.Broadway.NatsProducer

  defmodule FakeJetStream do
    @behaviour Quanta.Nats.JetStream.Behaviour

    @impl true
    def consumer_create(_stream, _subject_filter, _start_seq) do
      {:ok, make_ref()}
    end

    @impl true
    def consumer_fetch(_consumer, batch_size, _timeout_ms) do
      messages =
        for i <- 1..min(batch_size, 3) do
          %{
            subject: "quanta.test_ns.evt.test_type.actor_#{i}",
            payload: "payload_#{i}",
            seq: i
          }
        end

      {:ok, messages}
    end

    @impl true
    def consumer_delete(_consumer), do: :ok

    @impl true
    def publish(_subject, _payload, _seq), do: {:ok, %{stream: "test", seq: 1}}

    @impl true
    def kv_get(_bucket, _key), do: {:error, :not_found}

    @impl true
    def kv_put(_bucket, _key, _value), do: {:ok, 1}

    @impl true
    def kv_delete(_bucket, _key), do: :ok

    @impl true
    def purge_subject(_stream, _subject), do: :ok
  end

  defmodule ErrorJetStream do
    @behaviour Quanta.Nats.JetStream.Behaviour

    @impl true
    def consumer_create(_stream, _subject_filter, _start_seq) do
      {:ok, make_ref()}
    end

    @impl true
    def consumer_fetch(_consumer, _batch_size, _timeout_ms) do
      {:error, :timeout}
    end

    @impl true
    def consumer_delete(_consumer), do: :ok

    @impl true
    def publish(_subject, _payload, _seq), do: {:ok, %{stream: "test", seq: 1}}

    @impl true
    def kv_get(_bucket, _key), do: {:error, :not_found}

    @impl true
    def kv_put(_bucket, _key, _value), do: {:ok, 1}

    @impl true
    def kv_delete(_bucket, _key), do: :ok

    @impl true
    def purge_subject(_stream, _subject), do: :ok
  end

  setup do
    prev = Application.get_env(:quanta_distributed, :jetstream_impl)
    Application.put_env(:quanta_distributed, :jetstream_impl, FakeJetStream)

    on_exit(fn ->
      if prev do
        Application.put_env(:quanta_distributed, :jetstream_impl, prev)
      else
        Application.delete_env(:quanta_distributed, :jetstream_impl)
      end
    end)

    :ok
  end

  describe "init/1" do
    test "starts as a producer with consumer_ref in state" do
      opts = [stream_name: "TEST", subject_filter: "quanta.test.evt.>"]
      assert {:producer, state} = NatsProducer.init(opts)
      assert is_reference(state.consumer_ref)
      assert state.demand == 0
      assert state.batch_size == 256
      assert state.receive_timeout == 5_000
    end

    test "respects custom fetch_batch_size and receive_timeout" do
      opts = [
        stream_name: "TEST",
        subject_filter: "quanta.test.evt.>",
        fetch_batch_size: 50,
        receive_timeout: 10_000
      ]

      assert {:producer, state} = NatsProducer.init(opts)
      assert state.batch_size == 50
      assert state.receive_timeout == 10_000
    end
  end

  describe "handle_demand/2" do
    test "accumulates demand and sends :fetch" do
      state = %{demand: 0, consumer_ref: make_ref(), batch_size: 256, receive_timeout: 5_000}
      assert {:noreply, [], new_state} = NatsProducer.handle_demand(10, state)
      assert new_state.demand == 10

      assert_receive :fetch
    end

    test "accumulates multiple demands" do
      state = %{demand: 5, consumer_ref: make_ref(), batch_size: 256, receive_timeout: 5_000}
      assert {:noreply, [], new_state} = NatsProducer.handle_demand(10, state)
      assert new_state.demand == 15
    end
  end

  describe "handle_info(:fetch, ...)" do
    test "returns empty when demand is 0" do
      state = %{demand: 0, consumer_ref: make_ref(), batch_size: 256, receive_timeout: 5_000}
      assert {:noreply, [], ^state} = NatsProducer.handle_info(:fetch, state)
    end

    test "fetches and wraps messages as Broadway.Message structs" do
      state = %{
        demand: 10,
        consumer_ref: make_ref(),
        stream_name: "TEST",
        batch_size: 256,
        receive_timeout: 5_000
      }

      assert {:noreply, messages, new_state} = NatsProducer.handle_info(:fetch, state)
      assert length(messages) == 3
      assert new_state.demand == 7

      [msg | _] = messages
      assert %Broadway.Message{} = msg
      assert msg.data == "payload_1"
      assert msg.metadata.subject == "quanta.test_ns.evt.test_type.actor_1"
      assert msg.metadata.seq == 1
      assert {Quanta.Broadway.NatsAcknowledger, :ack_ref, %{}} = msg.acknowledger
    end

    test "retries on fetch error with backoff" do
      Application.put_env(:quanta_distributed, :jetstream_impl, ErrorJetStream)

      state = %{
        demand: 10,
        consumer_ref: make_ref(),
        stream_name: "TEST",
        batch_size: 256,
        receive_timeout: 5_000
      }

      assert {:noreply, [], ^state} = NatsProducer.handle_info(:fetch, state)
      refute_receive :fetch, 100
      assert_receive :fetch, 2_000
    end
  end
end
