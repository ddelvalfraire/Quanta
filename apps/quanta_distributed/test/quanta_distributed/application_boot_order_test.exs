defmodule QuantaDistributed.ApplicationBootOrderTest do
  @moduledoc """
  Architectural test for MEDIUM-5.

  `QuantaDistributed.Application.start/2` historically called
  `:syn.add_node_to_scopes/1` and `:syn.set_event_handler/1` BEFORE
  `Supervisor.start_link`. If either raised (scope conflict on cluster rejoin,
  syn not yet started, etc.), the Application callback crashed and the node
  failed to boot.

  The fix moves syn setup into a dedicated supervised worker
  (`Quanta.SynConfig`) that performs the calls inside its `init/1`, supervised
  as the first child of `Quanta.Supervisor`. This test enforces that no
  `:syn.*` calls remain in the Application callback body.
  """

  use ExUnit.Case, async: true

  @application_source File.read!(
                        Path.expand(
                          "../../lib/quanta_distributed/application.ex",
                          __DIR__
                        )
                      )

  describe "boot order invariants (MEDIUM-5)" do
    test "application.ex does not call :syn.* before Supervisor.start_link" do
      # Any :syn.* call in the Application start/2 callback will crash the node
      # boot if syn raises (scope conflict, not yet started, etc.). Wrap the
      # calls in a dedicated supervised worker instead.
      refute @application_source =~ ":syn.add_node_to_scopes",
             "application.ex still calls :syn.add_node_to_scopes/1 in start/2; " <>
               "move it into a supervised worker (e.g. Quanta.SynConfig.init/1)."

      refute @application_source =~ ":syn.set_event_handler",
             "application.ex still calls :syn.set_event_handler/1 in start/2; " <>
               "move it into a supervised worker (e.g. Quanta.SynConfig.init/1)."
    end

    test "Quanta.SynConfig (or equivalent) worker module exists" do
      syn_config_path =
        Path.expand(
          "../../lib/quanta/syn_config.ex",
          __DIR__
        )

      assert File.exists?(syn_config_path),
             "expected a dedicated supervised worker at " <>
               "apps/quanta_distributed/lib/quanta/syn_config.ex that performs " <>
               ":syn.add_node_to_scopes/1 and :syn.set_event_handler/1 inside init/1."
    end
  end
end
