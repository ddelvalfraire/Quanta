defmodule Quanta.Nifs.NatsJetstreamTest do
  use ExUnit.Case, async: false

  @moduletag :nats

  setup_all do
    {:ok, conn} = Quanta.Nifs.Native.nats_connect(["nats://localhost:4222"], %{})
    %{conn: conn}
  end

  describe "nats_connect/2" do
    test "connects to a local NATS server" do
      assert {:ok, conn} = Quanta.Nifs.Native.nats_connect(["nats://localhost:4222"], %{})
      assert is_reference(conn)
    end

    test "connects with custom options" do
      opts = %{max_in_flight: 100, connect_timeout_ms: 2_000}
      assert {:ok, conn} = Quanta.Nifs.Native.nats_connect(["nats://localhost:4222"], opts)
      assert is_reference(conn)
    end

    test "returns error for unreachable server" do
      assert {:error, reason} =
               Quanta.Nifs.Native.nats_connect(["nats://localhost:19999"], %{connect_timeout_ms: 500})

      assert is_binary(reason)
      assert reason =~ "connect_error"
    end

    test "rejects negative max_in_flight" do
      # Negative values are rejected by try_from, falling back to default (10_000)
      assert {:ok, _conn} =
               Quanta.Nifs.Native.nats_connect(["nats://localhost:4222"], %{max_in_flight: -1})
    end
  end

  describe "js_publish_async/6" do
    setup do
      stream = "TEST_PUBLISH_#{:erlang.unique_integer([:positive])}"
      ensure_stream(stream, "test.publish.#{stream}.>")
      on_exit(fn -> delete_stream(stream) end)
      %{stream_name: stream}
    end

    test "publishes and receives ack", %{conn: conn, stream_name: stream} do
      ref = make_ref()
      subject = "test.publish.#{stream}.item1"

      assert :ok =
               Quanta.Nifs.Native.js_publish_async(conn, self(), ref, subject, "hello", nil)

      assert_receive {:ok, ^ref, %{stream: _, seq: seq}}, 5_000
      assert is_integer(seq)
    end

    test "wrong expected sequence returns error", %{conn: conn, stream_name: stream} do
      subject = "test.publish.#{stream}.item2"

      # Publish first message
      ref1 = make_ref()
      :ok = Quanta.Nifs.Native.js_publish_async(conn, self(), ref1, subject, "first", nil)
      assert_receive {:ok, ^ref1, %{seq: _}}, 5_000

      # Publish with wrong expected sequence
      ref2 = make_ref()
      :ok = Quanta.Nifs.Native.js_publish_async(conn, self(), ref2, subject, "second", 999)
      assert_receive {:error, ^ref2, :wrong_last_sequence}, 5_000
    end
  end

  describe "kv_get_async/5 and kv_put_async/6" do
    setup do
      bucket = "test_kv_#{:erlang.unique_integer([:positive])}"
      ensure_kv_bucket(bucket)
      on_exit(fn -> delete_kv_bucket(bucket) end)
      %{bucket: bucket}
    end

    test "put then get roundtrip", %{conn: conn, bucket: bucket} do
      # Put
      ref1 = make_ref()
      :ok = Quanta.Nifs.Native.kv_put_async(conn, self(), ref1, bucket, "mykey", "myvalue")
      assert_receive {:ok, ^ref1, %{revision: rev}}, 5_000
      assert is_integer(rev)

      # Get
      ref2 = make_ref()
      :ok = Quanta.Nifs.Native.kv_get_async(conn, self(), ref2, bucket, "mykey")
      assert_receive {:ok, ^ref2, %{value: value, revision: ^rev}}, 5_000
      assert value == "myvalue"
    end

    test "get missing key returns not_found", %{conn: conn, bucket: bucket} do
      ref = make_ref()
      :ok = Quanta.Nifs.Native.kv_get_async(conn, self(), ref, bucket, "nonexistent")
      assert_receive {:error, ^ref, :not_found}, 5_000
    end
  end

  describe "kv_delete_async/5" do
    setup do
      bucket = "test_kvdel_#{:erlang.unique_integer([:positive])}"
      ensure_kv_bucket(bucket)
      on_exit(fn -> delete_kv_bucket(bucket) end)
      %{bucket: bucket}
    end

    test "delete a key", %{conn: conn, bucket: bucket} do
      # Put first
      ref1 = make_ref()
      :ok = Quanta.Nifs.Native.kv_put_async(conn, self(), ref1, bucket, "delme", "val")
      assert_receive {:ok, ^ref1, _}, 5_000

      # Delete
      ref2 = make_ref()
      :ok = Quanta.Nifs.Native.kv_delete_async(conn, self(), ref2, bucket, "delme")
      assert_receive {:ok, ^ref2}, 5_000

      # Verify gone
      ref3 = make_ref()
      :ok = Quanta.Nifs.Native.kv_get_async(conn, self(), ref3, bucket, "delme")
      assert_receive {:error, ^ref3, :not_found}, 5_000
    end
  end

  describe "consumer lifecycle" do
    setup do
      stream = "TEST_CONSUMER_#{:erlang.unique_integer([:positive])}"
      ensure_stream(stream, "test.consumer.#{stream}.>")
      on_exit(fn -> delete_stream(stream) end)
      %{stream_name: stream}
    end

    test "create, fetch, delete", %{conn: conn, stream_name: stream} do
      subject = "test.consumer.#{stream}.events"

      # Publish 3 messages
      for i <- 1..3 do
        ref = make_ref()
        :ok = Quanta.Nifs.Native.js_publish_async(conn, self(), ref, subject, "msg#{i}", nil)
        assert_receive {:ok, ^ref, _}, 5_000
      end

      # Create consumer
      ref_create = make_ref()
      :ok = Quanta.Nifs.Native.consumer_create_async(conn, self(), ref_create, stream, subject, 0)
      assert_receive {:ok, ^ref_create, consumer_ref}, 5_000
      assert is_reference(consumer_ref)

      # Fetch messages
      ref_fetch = make_ref()
      :ok = Quanta.Nifs.Native.consumer_fetch_async(conn, self(), ref_fetch, consumer_ref, 10, 2_000)
      assert_receive {:ok, ^ref_fetch, messages}, 5_000
      assert length(messages) == 3

      [msg1, msg2, msg3] = messages
      assert msg1.subject == subject
      assert msg1.payload == "msg1"
      assert is_integer(msg1.seq)
      assert msg2.payload == "msg2"
      assert msg3.payload == "msg3"

      # Delete consumer
      ref_del = make_ref()
      :ok = Quanta.Nifs.Native.consumer_delete_async(conn, self(), ref_del, consumer_ref)
      assert_receive {:ok, ^ref_del}, 5_000
    end
  end

  describe "purge_subject_async/5" do
    setup do
      stream = "TEST_PURGE_#{:erlang.unique_integer([:positive])}"
      ensure_stream(stream, "test.purge.#{stream}.>")
      on_exit(fn -> delete_stream(stream) end)
      %{stream_name: stream}
    end

    test "purges messages for a subject", %{conn: conn, stream_name: stream} do
      subject = "test.purge.#{stream}.item"

      # Publish messages
      for i <- 1..3 do
        ref = make_ref()
        :ok = Quanta.Nifs.Native.js_publish_async(conn, self(), ref, subject, "msg#{i}", nil)
        assert_receive {:ok, ^ref, _}, 5_000
      end

      # Purge
      ref_purge = make_ref()
      :ok = Quanta.Nifs.Native.purge_subject_async(conn, self(), ref_purge, stream, subject)
      assert_receive {:ok, ^ref_purge}, 5_000

      # Verify: create consumer at seq 0, fetch should get nothing
      ref_create = make_ref()
      :ok = Quanta.Nifs.Native.consumer_create_async(conn, self(), ref_create, stream, subject, 0)
      assert_receive {:ok, ^ref_create, consumer_ref}, 5_000

      ref_fetch = make_ref()
      :ok = Quanta.Nifs.Native.consumer_fetch_async(conn, self(), ref_fetch, consumer_ref, 10, 1_000)
      assert_receive {:ok, ^ref_fetch, messages}, 5_000
      assert messages == []

      # Cleanup
      ref_del = make_ref()
      :ok = Quanta.Nifs.Native.consumer_delete_async(conn, self(), ref_del, consumer_ref)
      assert_receive {:ok, ^ref_del}, 5_000
    end
  end

  describe "backpressure" do
    test "returns :nats_backpressure when semaphore is full" do
      {:ok, conn} = Quanta.Nifs.Native.nats_connect(["nats://localhost:4222"], %{max_in_flight: 1})

      stream = "TEST_BP_#{:erlang.unique_integer([:positive])}"
      ensure_stream(stream, "test.bp.#{stream}.>")
      on_exit(fn -> delete_stream(stream) end)

      subject = "test.bp.#{stream}.item"

      # Fire many calls rapidly — with max_in_flight: 1, at least some should
      # get backpressure since the Tokio task holds the permit until it completes.
      results =
        for i <- 1..50 do
          ref = make_ref()
          Quanta.Nifs.Native.js_publish_async(conn, self(), ref, subject, "msg#{i}", nil)
        end

      backpressure_count = Enum.count(results, &(&1 == {:error, :nats_backpressure}))
      ok_count = Enum.count(results, &(&1 == :ok))

      assert backpressure_count > 0,
        "Expected at least one backpressure response, got #{ok_count} oks"

      # Drain all successful publish acks
      for _ <- 1..ok_count do
        assert_receive {:ok, _, _}, 5_000
      end
    end
  end

  # --- Test helpers ---

  defp ensure_stream(stream_name, subjects) do
    {:ok, gnat} = Gnat.start_link(%{host: "localhost", port: 4222})

    payload =
      Jason.encode!(%{
        name: stream_name,
        subjects: [subjects],
        retention: "limits",
        storage: "memory",
        max_msgs: 1000
      })

    {:ok, %{body: _}} = Gnat.request(gnat, "$JS.API.STREAM.CREATE.#{stream_name}", payload)
    GenServer.stop(gnat)
  end

  defp delete_stream(stream_name) do
    {:ok, gnat} = Gnat.start_link(%{host: "localhost", port: 4222})
    Gnat.request(gnat, "$JS.API.STREAM.DELETE.#{stream_name}", "")
    GenServer.stop(gnat)
  rescue
    _ -> :ok
  end

  defp ensure_kv_bucket(bucket_name) do
    {:ok, gnat} = Gnat.start_link(%{host: "localhost", port: 4222})

    payload =
      Jason.encode!(%{
        name: "KV_#{bucket_name}",
        subjects: ["$KV.#{bucket_name}.>"],
        retention: "limits",
        storage: "memory",
        max_msgs_per_subject: 1,
        discard: "new",
        allow_rollup_hdrs: true,
        deny_delete: true,
        deny_purge: false,
        num_replicas: 1
      })

    {:ok, %{body: _}} = Gnat.request(gnat, "$JS.API.STREAM.CREATE.KV_#{bucket_name}", payload)
    GenServer.stop(gnat)
  end

  defp delete_kv_bucket(bucket_name) do
    {:ok, gnat} = Gnat.start_link(%{host: "localhost", port: 4222})
    Gnat.request(gnat, "$JS.API.STREAM.DELETE.KV_#{bucket_name}", "")
    GenServer.stop(gnat)
  rescue
    _ -> :ok
  end
end
