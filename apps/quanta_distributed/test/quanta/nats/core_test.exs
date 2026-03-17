defmodule Quanta.Nats.CoreTest do
  use ExUnit.Case, async: true

  alias Quanta.Nats.{Core, CoreSupervisor}

  describe "CoreSupervisor.parse_nats_url/1" do
    test "parses standard nats:// URL" do
      assert %{host: ~c"myhost", port: 4222} =
               CoreSupervisor.parse_nats_url("nats://myhost:4222")
    end

    test "defaults port to 4222 when omitted" do
      assert %{host: ~c"myhost", port: 4222} =
               CoreSupervisor.parse_nats_url("nats://myhost")
    end

    test "parses non-default port" do
      assert %{host: ~c"nats.example.com", port: 5555} =
               CoreSupervisor.parse_nats_url("nats://nats.example.com:5555")
    end

    test "handles URL without scheme" do
      assert %{host: ~c"bare-host", port: 4222} =
               CoreSupervisor.parse_nats_url("bare-host")
    end

    test "handles host:port without scheme" do
      assert %{host: ~c"bare-host", port: 9999} =
               CoreSupervisor.parse_nats_url("bare-host:9999")
    end

    test "raises on URL with credentials" do
      assert_raise ArgumentError, ~r/must not contain credentials/, fn ->
        CoreSupervisor.parse_nats_url("nats://user:pass@host:4222")
      end
    end

    test "merges base_opts into result" do
      result = CoreSupervisor.parse_nats_url("nats://myhost:4222", %{tls: true})
      assert result == %{host: ~c"myhost", port: 4222, tls: true}
    end
  end

  describe "Core.connection/1" do
    test "returns atom name for index" do
      assert Core.connection(0) == :quanta_nats_0
      assert Core.connection(3) == :quanta_nats_3
    end
  end

  describe "Core.connection/0" do
    test "returns a pool atom name" do
      conn = Core.connection()
      assert is_atom(conn)
      assert conn |> Atom.to_string() |> String.starts_with?("quanta_nats_")
    end

    test "returns deterministic result for same process" do
      assert Core.connection() == Core.connection()
    end
  end

  describe "Core.pool_size/0" do
    test "reads configured pool size" do
      prev = Application.get_env(:quanta_distributed, :nats_pool_size)
      Application.put_env(:quanta_distributed, :nats_pool_size, 5)

      assert Core.pool_size() == 5

      if prev, do: Application.put_env(:quanta_distributed, :nats_pool_size, prev)
    end

    test "defaults to 2" do
      prev = Application.get_env(:quanta_distributed, :nats_pool_size)
      Application.delete_env(:quanta_distributed, :nats_pool_size)

      assert Core.pool_size() == 2

      if prev, do: Application.put_env(:quanta_distributed, :nats_pool_size, prev)
    end
  end

  # --- Integration tests (require local NATS on :4222) ---

  describe "publish + subscribe round-trip" do
    @describetag :nats

    test "subscriber receives published message" do
      subject = "quanta.test.core.#{System.unique_integer([:positive])}"

      {:ok, {conn, sid}} = Core.subscribe(subject, "test-group", self())
      # Allow SUB to propagate to NATS server before publishing
      Process.sleep(50)
      Core.publish(subject, "hello")

      assert_receive {:msg, %{topic: ^subject, body: "hello"}}, 2_000

      Core.unsubscribe({conn, sid})
    end
  end

  describe "request-reply" do
    @describetag :nats

    test "request receives a reply" do
      subject = "quanta.test.rr.#{System.unique_integer([:positive])}"

      # Start a responder process that subscribes and echoes back
      {:ok, _responder} =
        Task.start_link(fn ->
          {:ok, sub} = Core.subscribe(subject, nil, self())

          receive do
            {:msg, %{reply_to: reply_to, body: body}} ->
              Core.publish(reply_to, "echo:#{body}")
          end

          Core.unsubscribe(sub)
        end)

      # Give the responder time to subscribe
      Process.sleep(50)

      {:ok, reply} = Core.request(subject, "ping", 2_000)
      assert reply.body == "echo:ping"
    end
  end

  describe "unsubscribe" do
    @describetag :nats

    test "stops delivery after unsubscribe" do
      subject = "quanta.test.unsub.#{System.unique_integer([:positive])}"

      {:ok, sub} = Core.subscribe(subject, "test-group", self())
      Process.sleep(50)

      Core.publish(subject, "before")
      assert_receive {:msg, %{body: "before"}}, 2_000

      Core.unsubscribe(sub)
      Core.publish(subject, "after")
      refute_receive {:msg, %{body: "after"}}, 500
    end
  end
end
