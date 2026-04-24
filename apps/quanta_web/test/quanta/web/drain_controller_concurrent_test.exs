defmodule Quanta.Web.DrainControllerConcurrentTest do
  use Quanta.Web.ConnCase, async: false

  # HIGH-4: Drain.start_drain/1 uses `GenServer.start(__MODULE__, ..., name: __MODULE__)`.
  # A second HTTP POST /api/internal/drain while the first is in progress hits the
  # `{:error, {:already_started, _pid}}` branch in DrainController (drain_controller.ex:22).
  #
  # DrainController handles that branch by calling await_and_respond/1, which calls
  # Quanta.Drain.await/1.  Drain.await/1 does:
  #
  #     receive do
  #       {:drain_complete, Quanta.Drain} -> :ok
  #     after timeout -> :timeout
  #     end
  #
  # But the GenServer only sends {:drain_complete, __MODULE__} to state.caller —
  # the pid that called start_drain/1 in the FIRST request's process.  The SECOND
  # request's process calls await/1 but will never receive that message; it times
  # out and the controller returns 504 instead of 200/202.
  #
  # The unit-level test below calls Drain.start_drain/1 + Drain.await/1 directly
  # from two different processes to isolate the bug without needing to wait 95 s
  # for the controller-level timeout.  The second process calling await/1 should
  # return :ok (fixed) but returns :timeout (bug present).

  @internal_token "test-internal-token"

  @fast_drain_opts [
    complete_in_flight_delay_ms: 50,
    ordered_passivation_delay_ms: 50,
    force_stop_delay_ms: 500
  ]

  setup do
    Application.put_env(:quanta_web, :drain_opts, @fast_drain_opts)
    Application.put_env(:quanta_web, :internal_auth_token, @internal_token)

    on_exit(fn ->
      Application.delete_env(:quanta_web, :drain_opts)
      Application.delete_env(:quanta_web, :internal_auth_token)

      try do
        :persistent_term.erase({Quanta.Drain, :draining})
      rescue
        ArgumentError -> :ok
      end

      if pid = Process.whereis(Quanta.Drain) do
        try do
          GenServer.stop(pid, :normal, 2_000)
        catch
          _, _ -> :ok
        end
      end

      if Process.whereis(Quanta.Cluster.Topology) do
        send(Process.whereis(Quanta.Cluster.Topology), {:nodeup, node(), []})
        Quanta.Cluster.Topology.nodes()
      end
    end)

    :ok
  end

  describe "HIGH-4: second drain caller never receives {:drain_complete, _}" do
    test "Drain.start_drain/1 called twice returns {:error, {:already_started, _}} on second call" do
      # Precondition: the raw GenServer behaviour that causes the bug.
      {:ok, _pid} = Quanta.Drain.start_drain(@fast_drain_opts)
      assert {:error, {:already_started, _}} = Quanta.Drain.start_drain(@fast_drain_opts)
    end

    @tag timeout: 10_000
    test "second process calling Drain.await/1 returns :ok — currently times out" do
      # Process 1 starts the drain and is registered as state.caller.
      # It will receive {:drain_complete, Quanta.Drain} when the drain finishes.
      test_pid = self()

      task1 =
        Task.async(fn ->
          {:ok, _pid} = Quanta.Drain.start_drain(@fast_drain_opts)
          # Signal that start_drain has been called so task2 can proceed.
          send(test_pid, :drain_started)
          Quanta.Drain.await(5_000)
        end)

      # Wait until task1 has called start_drain before task2 tries.
      assert_receive :drain_started, 2_000

      # Process 2 hits {:already_started, _} in start_drain and must fall
      # back to await/1.  With the bug, it blocks forever because
      # {:drain_complete, Quanta.Drain} is only sent to task1's pid.
      task2 =
        Task.async(fn ->
          # Mirrors what DrainController does in the already_started branch.
          case Quanta.Drain.start_drain(@fast_drain_opts) do
            {:ok, _pid} ->
              Quanta.Drain.await(5_000)

            {:error, {:already_started, _}} ->
              # BUG: await/1 here will never unblock; it receives nothing.
              Quanta.Drain.await(5_000)
          end
        end)

      result1 = Task.await(task1, 6_000)

      # BUG: task2 never receives {:drain_complete, Quanta.Drain} and returns
      # :timeout.  A correct implementation would broadcast to all waiters or
      # use a different notification mechanism, and task2 would return :ok.
      result2 =
        try do
          Task.await(task2, 6_000)
        catch
          :exit, {:timeout, _} -> :task_timeout
        end

      assert result1 == :ok,
             "First drain caller should complete with :ok, got: #{inspect(result1)}"

      # This is the failing assertion that proves HIGH-4.
      assert result2 == :ok,
             "Second concurrent drain caller should also return :ok, " <>
               "got: #{inspect(result2)} (bug: {:drain_complete} sent only to first caller's pid)"
    end
  end
end
