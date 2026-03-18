defmodule Quanta.Nats.JetStreamTest do
  use ExUnit.Case, async: false

  alias Quanta.Nats.JetStream
  import Quanta.Test.NatsHelpers

  describe "get_connection/0" do
    test "returns {:error, :not_connected} when persistent_term is not set" do
      # Save existing connection, erase, test, then restore
      prev = Quanta.Nats.JetStream.Connection.get_connection()

      :persistent_term.erase(:quanta_jetstream_conn)

      try do
        assert {:error, :not_connected} =
                 Quanta.Nats.JetStream.Connection.get_connection()
      after
        case prev do
          {:ok, conn} -> :persistent_term.put(:quanta_jetstream_conn, conn)
          {:error, :not_connected} -> :ok
        end
      end
    end
  end

  describe "publish/3" do
    @describetag :nats

    setup do
      stream = "TEST_JS_PUB_#{:erlang.unique_integer([:positive])}"
      subject_prefix = "test.jspub.#{stream}"
      ensure_stream(stream, "#{subject_prefix}.>")
      on_exit(fn -> delete_stream(stream) end)
      %{stream_name: stream, subject_prefix: subject_prefix}
    end

    test "returns {:ok, ack} with stream and seq", %{subject_prefix: prefix} do
      assert {:ok, %{stream: stream, seq: seq}} =
               JetStream.publish("#{prefix}.a", "hello")

      assert is_binary(stream)
      assert is_integer(seq)
    end

    test "returns {:error, :wrong_last_sequence} on mismatch", %{subject_prefix: prefix} do
      subject = "#{prefix}.b"
      assert {:ok, _} = JetStream.publish(subject, "first")
      assert {:error, :wrong_last_sequence} = JetStream.publish(subject, "second", 999)
    end
  end

  describe "kv_put/3 and kv_get/2" do
    @describetag :nats

    setup do
      bucket = "test_jskv_#{:erlang.unique_integer([:positive])}"
      ensure_kv_bucket(bucket)
      on_exit(fn -> delete_kv_bucket(bucket) end)
      %{bucket: bucket}
    end

    test "roundtrip put then get", %{bucket: bucket} do
      assert {:ok, rev} = JetStream.kv_put(bucket, "mykey", "myvalue")
      assert is_integer(rev)

      assert {:ok, "myvalue", ^rev} = JetStream.kv_get(bucket, "mykey")
    end

    test "get missing key returns {:error, :not_found}", %{bucket: bucket} do
      assert {:error, :not_found} = JetStream.kv_get(bucket, "nonexistent")
    end
  end

  describe "kv_delete/2" do
    @describetag :nats

    setup do
      bucket = "test_jskv_del_#{:erlang.unique_integer([:positive])}"
      ensure_kv_bucket(bucket)
      on_exit(fn -> delete_kv_bucket(bucket) end)
      %{bucket: bucket}
    end

    test "delete then get returns :not_found", %{bucket: bucket} do
      assert {:ok, _} = JetStream.kv_put(bucket, "delme", "val")
      assert :ok = JetStream.kv_delete(bucket, "delme")
      assert {:error, :not_found} = JetStream.kv_get(bucket, "delme")
    end
  end

  describe "consumer lifecycle" do
    @describetag :nats

    setup do
      stream = "TEST_JS_CON_#{:erlang.unique_integer([:positive])}"
      subject_prefix = "test.jscon.#{stream}"
      ensure_stream(stream, "#{subject_prefix}.>")
      on_exit(fn -> delete_stream(stream) end)
      %{stream_name: stream, subject_prefix: subject_prefix}
    end

    test "create, fetch, delete", %{stream_name: stream, subject_prefix: prefix} do
      subject = "#{prefix}.events"

      # Publish 3 messages
      for i <- 1..3 do
        assert {:ok, _} = JetStream.publish(subject, "msg#{i}")
      end

      # Create consumer
      assert {:ok, consumer} = JetStream.consumer_create(stream, subject, 0)
      assert is_reference(consumer)

      # Fetch
      assert {:ok, messages} = JetStream.consumer_fetch(consumer, 10, 2_000)
      assert length(messages) == 3
      assert Enum.map(messages, & &1.payload) == ["msg1", "msg2", "msg3"]

      # Delete
      assert :ok = JetStream.consumer_delete(consumer)
    end
  end

  describe "purge_subject/2" do
    @describetag :nats

    setup do
      stream = "TEST_JS_PURGE_#{:erlang.unique_integer([:positive])}"
      subject_prefix = "test.jspurge.#{stream}"
      ensure_stream(stream, "#{subject_prefix}.>")
      on_exit(fn -> delete_stream(stream) end)
      %{stream_name: stream, subject_prefix: subject_prefix}
    end

    test "purge then fetch returns empty", %{stream_name: stream, subject_prefix: prefix} do
      subject = "#{prefix}.item"

      # Publish
      for i <- 1..3 do
        assert {:ok, _} = JetStream.publish(subject, "msg#{i}")
      end

      # Purge
      assert :ok = JetStream.purge_subject(stream, subject)

      # Verify empty
      assert {:ok, consumer} = JetStream.consumer_create(stream, subject, 0)
      assert {:ok, []} = JetStream.consumer_fetch(consumer, 10, 1_000)
      assert :ok = JetStream.consumer_delete(consumer)
    end
  end
end
