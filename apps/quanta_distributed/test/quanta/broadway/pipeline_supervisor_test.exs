defmodule Quanta.Broadway.PipelineSupervisorTest do
  use ExUnit.Case, async: false

  alias Quanta.Broadway.PipelineSupervisor

  defmodule FakeJetStream do
    @behaviour Quanta.Nats.JetStream.Behaviour

    @impl true
    def consumer_create(_stream, _subject_filter, _start_seq), do: {:ok, make_ref()}
    @impl true
    def consumer_fetch(_consumer, _batch_size, _timeout_ms), do: {:ok, []}
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

  describe "start_pipeline/3" do
    test "starts a Broadway pipeline" do
      namespace = "test_#{:erlang.unique_integer([:positive])}"

      assert {:ok, pid} =
               PipelineSupervisor.start_pipeline(namespace, "events",
                 stream_name: "TEST_STREAM",
                 subject_filter: "quanta.#{namespace}.evt.events.>"
               )

      assert Process.alive?(pid)
      Broadway.stop(:"broadway_#{namespace}_events")
    end

    test "returns error when pipeline already exists" do
      namespace = "test_#{:erlang.unique_integer([:positive])}"

      {:ok, _pid} =
        PipelineSupervisor.start_pipeline(namespace, "events",
          stream_name: "TEST_STREAM",
          subject_filter: "quanta.#{namespace}.evt.events.>"
        )

      assert {:error, _} =
               PipelineSupervisor.start_pipeline(namespace, "events",
                 stream_name: "TEST_STREAM",
                 subject_filter: "quanta.#{namespace}.evt.events.>"
               )

      Broadway.stop(:"broadway_#{namespace}_events")
    end
  end

  describe "stop_pipeline/2" do
    test "stops a running pipeline" do
      namespace = "test_#{:erlang.unique_integer([:positive])}"

      {:ok, _pid} =
        PipelineSupervisor.start_pipeline(namespace, "events",
          stream_name: "TEST_STREAM",
          subject_filter: "quanta.#{namespace}.evt.events.>"
        )

      assert :ok = PipelineSupervisor.stop_pipeline(namespace, "events")
    end

    test "returns error when pipeline does not exist" do
      assert {:error, :not_found} = PipelineSupervisor.stop_pipeline("nonexistent", "nope")
    end
  end

  describe "list_pipelines/0" do
    test "lists running pipelines" do
      namespace = "test_#{:erlang.unique_integer([:positive])}"

      initial_count = length(PipelineSupervisor.list_pipelines())

      {:ok, _pid} =
        PipelineSupervisor.start_pipeline(namespace, "events",
          stream_name: "TEST_STREAM",
          subject_filter: "quanta.#{namespace}.evt.events.>"
        )

      assert length(PipelineSupervisor.list_pipelines()) == initial_count + 1

      Broadway.stop(:"broadway_#{namespace}_events")
    end
  end
end
