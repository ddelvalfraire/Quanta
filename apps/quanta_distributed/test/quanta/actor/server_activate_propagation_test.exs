defmodule Quanta.Actor.ServerActivatePropagationTest do
  @moduledoc """
  Architectural test for MEDIUM-1.

  `Quanta.Actor.Server.activate/2` historically wrapped the activation path in a
  `try/rescue` and used a bespoke `@init_attempts_table` ETS counter to stop
  failing activations with `:normal` after 3 attempts. That masked the real
  crash reason from the supervisor and duplicated the restart-limit logic the
  supervisor (`:transient` + `max_restarts`/`max_seconds`) already provides.

  This test enforces the following invariants statically:

  (a) `activate/2` (and its helpers) must not contain a `try/rescue` that
      swallows exceptions from the activation path.
  (b) No `@init_attempts_table` / `:quanta_actor_init_attempts` ETS table is
      used as a bespoke retry counter — the supervisor is the single source of
      restart limits.
  (c) The server must not stop with `:normal` after a bespoke-counter
      threshold; activation failures must propagate as the real crash reason.
  """

  use ExUnit.Case, async: true

  @server_source File.read!(
                   Path.expand(
                     "../../../lib/quanta/actor/server.ex",
                     __DIR__
                   )
                 )

  @supervisor_source File.read!(
                       Path.expand(
                         "../../../lib/quanta/supervisor.ex",
                         __DIR__
                       )
                     )

  describe "activation crash propagation (MEDIUM-1)" do
    test "server.ex does not define a bespoke @init_attempts_table" do
      refute @server_source =~ "@init_attempts_table",
             "server.ex still references @init_attempts_table — the supervisor " <>
               "already bounds restart attempts via :transient + max_restarts; " <>
               "delete the bespoke ETS counter."
    end

    test "server.ex does not reference the quanta_actor_init_attempts ETS table" do
      refute @server_source =~ "quanta_actor_init_attempts",
             "server.ex still references :quanta_actor_init_attempts — remove the " <>
               "bespoke retry-counter ETS table."
    end

    test "supervisor.ex does not create the bespoke init-attempts ETS table" do
      refute @supervisor_source =~ "quanta_actor_init_attempts",
             "supervisor.ex still creates :quanta_actor_init_attempts — remove it; " <>
               "restart limits belong to the supervisor strategy."
    end

    test "server.ex does not wrap activation in a try/rescue catch-all" do
      # The activation path should let exceptions propagate so the supervisor
      # sees the real reason. Any `rescue` clause inside `defp activate/2`
      # masks the crash.
      refute Regex.match?(~r/defp activate\(.*?rescue/s, @server_source),
             "defp activate/2 still contains a rescue clause — let exceptions " <>
               "propagate so the supervisor logs the real crash reason."
    end

    test "server.ex does not stop with :normal after a bespoke init-failure counter" do
      refute @server_source =~ "failed init 3 times",
             "server.ex still logs the bespoke 3-strike retry message — remove " <>
               "`handle_init_failure` and let the supervisor bound restarts."

      refute @server_source =~ "defp handle_init_failure",
             "server.ex still defines `handle_init_failure/3` — remove it; " <>
               "activation failures must propagate as the real crash reason."
    end
  end
end
