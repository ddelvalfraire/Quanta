defmodule Quanta.Actor.SchemaRegistryTest do
  use ExUnit.Case, async: false

  alias Quanta.Actor.SchemaRegistry

  defmodule MockJetStream do
    @moduledoc false
    @behaviour Quanta.Nats.JetStream.Behaviour

    def start(initial_state \\ %{}) do
      {:ok, pid} = Agent.start_link(fn -> initial_state end, name: __MODULE__)
      pid
    end

    def stop do
      if Process.whereis(__MODULE__), do: Agent.stop(__MODULE__)
    end

    # --- KV operations backed by Agent state ---

    @impl true
    def kv_get(bucket, key) do
      Agent.get(__MODULE__, fn state ->
        case get_in(state, [bucket, key]) do
          nil -> {:error, :not_found}
          {value, revision} -> {:ok, value, revision}
        end
      end)
    end

    @impl true
    def kv_put(bucket, key, value) do
      Agent.get_and_update(__MODULE__, fn state ->
        bucket_data = Map.get(state, bucket, %{})
        revision = map_size(bucket_data) + 1

        new_state = put_in(state, [Access.key(bucket, %{}), key], {value, revision})
        {{:ok, revision}, new_state}
      end)
    end

    @impl true
    def kv_delete(bucket, key) do
      Agent.update(__MODULE__, fn state ->
        case state[bucket] do
          nil -> state
          bucket_data -> put_in(state, [bucket], Map.delete(bucket_data, key))
        end
      end)

      :ok
    end

    # --- Unused callbacks (required by behaviour) ---

    @impl true
    def publish(_subject, _payload, _seq), do: {:ok, %{stream: "test", seq: 1}}

    @impl true
    def consumer_create(_stream, _subject_filter, _start_seq), do: {:ok, make_ref()}

    @impl true
    def consumer_fetch(_consumer, _batch_size, _timeout_ms), do: {:ok, []}

    @impl true
    def consumer_delete(_consumer), do: :ok

    @impl true
    def purge_subject(_stream, _subject), do: :ok
  end

  setup do
    prev = Application.get_env(:quanta_distributed, :jetstream_impl)
    Application.put_env(:quanta_distributed, :jetstream_impl, MockJetStream)
    MockJetStream.start()

    on_exit(fn ->
      MockJetStream.stop()

      if prev do
        Application.put_env(:quanta_distributed, :jetstream_impl, prev)
      else
        Application.delete_env(:quanta_distributed, :jetstream_impl)
      end
    end)

    :ok
  end

  @namespace "test_ns"
  @type_name "counter"
  @wit_source "record state { count: u32 }"
  @compiled_bytes <<1, 2, 3, 4, 5>>

  describe "store and fetch" do
    test "store writes schema to KV, fetch retrieves it" do
      assert :ok = SchemaRegistry.store(@namespace, @type_name, 1, @wit_source, @compiled_bytes)
      assert {:ok, @compiled_bytes} = SchemaRegistry.fetch(@namespace, @type_name, 1)
    end

    test "two versions stored, both preserved" do
      other_source = "record state { count: u32, name: string }"
      other_bytes = <<6, 7, 8, 9>>

      assert :ok = SchemaRegistry.store(@namespace, @type_name, 1, @wit_source, @compiled_bytes)
      assert :ok = SchemaRegistry.store(@namespace, @type_name, 2, other_source, other_bytes)

      assert {:ok, @compiled_bytes} = SchemaRegistry.fetch(@namespace, @type_name, 1)
      assert {:ok, ^other_bytes} = SchemaRegistry.fetch(@namespace, @type_name, 2)
    end
  end

  describe "idempotency and immutability" do
    test "same version + same content is idempotent" do
      assert :ok = SchemaRegistry.store(@namespace, @type_name, 1, @wit_source, @compiled_bytes)
      assert :ok = SchemaRegistry.store(@namespace, @type_name, 1, @wit_source, @compiled_bytes)
    end

    test "same version + different content returns error" do
      assert :ok = SchemaRegistry.store(@namespace, @type_name, 1, @wit_source, @compiled_bytes)

      different_source = "record state { value: u64 }"
      different_bytes = <<10, 11, 12>>

      assert {:error, :immutability_violation, detail} =
               SchemaRegistry.store(@namespace, @type_name, 1, different_source, different_bytes)

      assert detail =~ "already exists with different content"
    end
  end

  describe "fetch missing" do
    test "fetch missing version returns {:error, :not_found}" do
      assert {:error, :not_found} = SchemaRegistry.fetch(@namespace, @type_name, 99)
    end
  end

  describe "version purging" do
    test "with max_versions: 2, storing version 3 deletes version 1" do
      prev = Application.get_env(:quanta_distributed, :schema_registry_max_versions)
      Application.put_env(:quanta_distributed, :schema_registry_max_versions, 2)

      on_exit(fn ->
        if prev do
          Application.put_env(:quanta_distributed, :schema_registry_max_versions, prev)
        else
          Application.delete_env(:quanta_distributed, :schema_registry_max_versions)
        end
      end)

      s1 = "record state { a: u32 }"
      s2 = "record state { a: u32, b: u32 }"
      s3 = "record state { a: u32, b: u32, c: u32 }"

      assert :ok = SchemaRegistry.store(@namespace, @type_name, 1, s1, <<1>>)
      assert :ok = SchemaRegistry.store(@namespace, @type_name, 2, s2, <<2>>)
      assert :ok = SchemaRegistry.store(@namespace, @type_name, 3, s3, <<3>>)

      # Version 1 was purged (3 - 2 = 1)
      assert {:error, :not_found} = SchemaRegistry.fetch(@namespace, @type_name, 1)

      # Versions 2 and 3 remain
      assert {:ok, <<2>>} = SchemaRegistry.fetch(@namespace, @type_name, 2)
      assert {:ok, <<3>>} = SchemaRegistry.fetch(@namespace, @type_name, 3)
    end
  end

  describe "value format" do
    test "raw KV value is <<sha256::32-bytes, compiled_bytes::rest>>" do
      assert :ok = SchemaRegistry.store(@namespace, @type_name, 1, @wit_source, @compiled_bytes)

      bucket = "quanta_#{@namespace}_schemas"
      key = "#{@type_name}:1"
      expected_hash = :crypto.hash(:sha256, @wit_source)
      compiled = @compiled_bytes

      {:ok, raw_value, _revision} = MockJetStream.kv_get(bucket, key)

      assert <<^expected_hash::binary-size(32), ^compiled::binary>> = raw_value
    end
  end
end
