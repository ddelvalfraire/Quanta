defmodule Quanta.RateLimitTest do
  use ExUnit.Case, async: false

  alias Quanta.RateLimit
  alias Quanta.{ActorId, Manifest}

  setup do
    # Ensure clean table for each test
    if :ets.whereis(Quanta.RateLimit) != :undefined do
      :ets.delete(Quanta.RateLimit)
    end

    RateLimit.init()
    :ok
  end

  defp actor_id(id \\ "a1") do
    %ActorId{namespace: "myapp", type: "counter", id: id}
  end

  defp manifest(per_instance, per_type \\ 100) do
    %Manifest{
      version: "1",
      type: "counter",
      namespace: "myapp",
      rate_limits: %Manifest.RateLimits{
        messages_per_second: per_instance,
        messages_per_second_type: per_type
      }
    }
  end

  describe "check/2 — per-instance limit" do
    test "allows up to messages_per_second" do
      m = manifest(5)
      aid = actor_id()

      for _ <- 1..5 do
        assert :ok = RateLimit.check(aid, m)
      end
    end

    test "rejects after limit is exhausted" do
      m = manifest(3)
      aid = actor_id()

      assert :ok = RateLimit.check(aid, m)
      assert :ok = RateLimit.check(aid, m)
      assert :ok = RateLimit.check(aid, m)
      assert {:error, :rate_limited, retry_ms} = RateLimit.check(aid, m)
      assert is_integer(retry_ms) and retry_ms > 0
    end

    test "different actor instances have independent limits" do
      m = manifest(2)

      for _ <- 1..2, do: assert(:ok = RateLimit.check(actor_id("a1"), m))
      assert {:error, :rate_limited, _} = RateLimit.check(actor_id("a1"), m)

      # a2 is unaffected
      assert :ok = RateLimit.check(actor_id("a2"), m)
    end
  end

  describe "check/2 — per-type aggregate limit" do
    test "rejects when type aggregate is exhausted" do
      # Per-instance limit of 10, but per-type limit of 3
      m = manifest(10, 3)

      assert :ok = RateLimit.check(actor_id("a1"), m)
      assert :ok = RateLimit.check(actor_id("a2"), m)
      assert :ok = RateLimit.check(actor_id("a3"), m)
      assert {:error, :rate_limited, _} = RateLimit.check(actor_id("a4"), m)
    end

    test "per-instance can reject before per-type" do
      # Per-instance limit of 1, per-type limit of 100
      m = manifest(1, 100)

      assert :ok = RateLimit.check(actor_id("a1"), m)
      assert {:error, :rate_limited, _} = RateLimit.check(actor_id("a1"), m)

      # Other instances still work (per-type not exhausted)
      assert :ok = RateLimit.check(actor_id("a2"), m)
    end
  end

  describe "check/2 — window reset" do
    test "tokens replenish after window expires" do
      m = manifest(1)
      aid = actor_id()

      assert :ok = RateLimit.check(aid, m)
      assert {:error, :rate_limited, _} = RateLimit.check(aid, m)

      # Simulate window expiry by manipulating the ETS entry
      key = {:instance, "myapp", "counter", "a1"}
      [{^key, _tokens, window_start}] = :ets.lookup(Quanta.RateLimit, key)
      # Set window_start to 2 seconds ago
      :ets.insert(Quanta.RateLimit, {key, 0, window_start - 2000})

      # Should now be allowed (new window)
      assert :ok = RateLimit.check(aid, m)
    end
  end

  describe "retry_after" do
    test "returns a positive integer" do
      m = manifest(1)
      aid = actor_id()

      assert :ok = RateLimit.check(aid, m)
      assert {:error, :rate_limited, retry_ms} = RateLimit.check(aid, m)
      assert is_integer(retry_ms)
      assert retry_ms > 0
      assert retry_ms <= 1000
    end
  end

  describe "reset/1" do
    test "clears counters for an actor" do
      m = manifest(1)
      aid = actor_id()

      assert :ok = RateLimit.check(aid, m)
      assert {:error, :rate_limited, _} = RateLimit.check(aid, m)

      RateLimit.reset(aid)

      assert :ok = RateLimit.check(aid, m)
    end
  end

  describe "init/0" do
    test "creates the ETS table" do
      assert :ets.whereis(Quanta.RateLimit) != :undefined
    end
  end

  describe "concurrent access" do
    test "handles concurrent checks without crashing" do
      m = manifest(1000, 100_000)
      aid = actor_id()

      tasks =
        for _ <- 1..100 do
          Task.async(fn -> RateLimit.check(aid, m) end)
        end

      results = Task.await_many(tasks)
      ok_count = Enum.count(results, &(&1 == :ok))
      assert ok_count > 0
      assert ok_count <= 1000
    end
  end
end
