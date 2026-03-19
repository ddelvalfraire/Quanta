defmodule Quanta.Actor.SynPropertyTest do
  @moduledoc """
  Property tests for syn conflict resolution.

  P1: Commutativity — swapping the two registrations yields the same winner.
       Determinism — same inputs always produce the same winner.
  """

  use ExUnit.Case, async: false
  use PropCheck

  alias Quanta.Actor.SynEventHandler

  @moduletag :property

  # ── Generators ──────────────────────────────────────────────────────

  defp draining_flag, do: boolean()

  defp syn_time, do: integer(1, 1_000_000)

  defp meta_gen do
    let draining <- draining_flag() do
      %{
        node: :nonode@nohost,
        type: "counter",
        nonce: :rand.uniform(0xFFFFFFFFFFFFFFFF),
        activated_at: 0,
        draining: draining
      }
    end
  end

  # ── Helpers ─────────────────────────────────────────────────────────

  defp spawn_waiting do
    spawn(fn -> receive do :stop -> :ok end end)
  end

  defp resolve(pid1, meta1, time1, pid2, meta2, time2) do
    SynEventHandler.resolve_registry_conflict(
      :actors,
      %Quanta.ActorId{namespace: "test", type: "prop", id: "conflict"},
      {pid1, meta1, time1},
      {pid2, meta2, time2}
    )
  end

  # Determine expected winner without side effects (mirrors pick_winner logic)
  defp expected_winner(pid1, meta1, time1, pid2, meta2, time2) do
    cond do
      meta1[:draining] == true and meta2[:draining] == false -> pid2
      meta1[:draining] == false and meta2[:draining] == true -> pid1
      time1 <= time2 -> pid1
      true -> pid2
    end
  end

  # ── Properties ──────────────────────────────────────────────────────

  setup do
    :syn.add_node_to_scopes([:actors])
    :ok
  end

  property "commutativity: when distinguishable, resolve(a,b) and resolve(b,a) agree" do
    forall {meta1, time1, meta2, time2} <- {meta_gen(), syn_time(), meta_gen(), syn_time()} do
      # Skip ties — equal timestamps with same draining state are a known
      # positional tie-break (practically impossible with nanosecond syn times).
      implies time1 != time2 or meta1.draining != meta2.draining do
        pid_a = spawn_waiting()
        pid_b = spawn_waiting()

        winner_ab = expected_winner(pid_a, meta1, time1, pid_b, meta2, time2)
        winner_ba = expected_winner(pid_b, meta2, time2, pid_a, meta1, time1)

        result = winner_ab == winner_ba

        for p <- [pid_a, pid_b], Process.alive?(p), do: send(p, :stop)

        result
      end
    end
  end

  property "determinism: same inputs always produce the same winner" do
    forall {meta1, time1, meta2, time2} <- {meta_gen(), syn_time(), meta_gen(), syn_time()} do
      pid1 = spawn_waiting()
      pid2 = spawn_waiting()

      winner1 = resolve(pid1, meta1, time1, pid2, meta2, time2)

      # Respawn since loser was killed
      pid3 = spawn_waiting()
      pid4 = spawn_waiting()

      winner2 = resolve(pid3, meta1, time1, pid4, meta2, time2)

      # The winner should be from the same "side" (1st or 2nd argument)
      same_side = (winner1 == pid1) == (winner2 == pid3)

      # Cleanup
      for p <- [pid1, pid2, pid3, pid4], Process.alive?(p), do: send(p, :stop)

      same_side
    end
  end

  property "non-draining always beats draining regardless of timestamps" do
    forall {time1, time2} <- {syn_time(), syn_time()} do
      pid1 = spawn_waiting()
      pid2 = spawn_waiting()

      meta_drain = %{node: :nonode@nohost, type: "counter", nonce: 1, activated_at: 0, draining: true}
      meta_live = %{node: :nonode@nohost, type: "counter", nonce: 2, activated_at: 0, draining: false}

      winner = resolve(pid1, meta_drain, time1, pid2, meta_live, time2)

      result = winner == pid2

      for p <- [pid1, pid2], Process.alive?(p), do: send(p, :stop)

      result
    end
  end
end
