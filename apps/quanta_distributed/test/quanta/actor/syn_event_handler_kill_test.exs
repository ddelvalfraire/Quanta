defmodule Quanta.Actor.SynEventHandlerKillTest do
  @moduledoc """
  Architectural test for MEDIUM-2.

  `SynEventHandler.resolve_registry_conflict/4` historically killed the losing
  pid with `Process.exit(loser, :kill)`. Because `:kill` is untrappable, the
  loser's `terminate/2` does not run — so ephemeral state in NATS KV is not
  persisted. Under activation-storm conflicts this produces data loss.

  Fixed code uses `:shutdown` (or an equivalent graceful stop) so the loser's
  `terminate/2` runs within the default shutdown timeout.
  """

  use ExUnit.Case, async: true

  @handler_source File.read!(
                    Path.expand(
                      "../../../lib/quanta/actor/syn_event_handler.ex",
                      __DIR__
                    )
                  )

  describe "graceful termination (MEDIUM-2)" do
    test "no Process.exit(_, :kill) in syn_event_handler.ex" do
      # :kill is untrappable — terminate/2 does not run, which skips persisting
      # ephemeral state. Use :shutdown or GenServer.stop(_, :normal) instead.
      refute Regex.match?(~r/Process\.exit\([^)]*,\s*:kill\s*\)/, @handler_source),
             "syn_event_handler.ex still calls Process.exit(_, :kill); use :shutdown " <>
               "(or GenServer.stop(_, :normal)) so terminate/2 runs and ephemeral " <>
               "state gets persisted."
    end

    test "moduledoc does not advertise :kill as the conflict-resolution strategy" do
      refute @handler_source =~ ":kill",
             "syn_event_handler.ex still mentions :kill; update moduledoc and " <>
               "implementation to reflect the graceful-shutdown strategy."
    end
  end
end
