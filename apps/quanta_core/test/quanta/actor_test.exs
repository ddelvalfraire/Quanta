defmodule Quanta.ActorTest do
  use ExUnit.Case, async: true

  defmodule TestActor do
    @behaviour Quanta.Actor

    @impl true
    def init(state), do: {state, []}

    @impl true
    def handle_message(state, _envelope), do: {state, []}

    @impl true
    def handle_timer(state, _name), do: {state, []}
  end

  defmodule TestActorWithPassivate do
    @behaviour Quanta.Actor

    @impl true
    def init(state), do: {state, []}

    @impl true
    def handle_message(state, _envelope), do: {state, []}

    @impl true
    def handle_timer(state, _name), do: {state, []}

    @impl true
    def on_passivate(state), do: state
  end

  test "minimal actor implementation satisfies behaviour" do
    assert {<<>>, []} == TestActor.init(<<>>)
  end

  test "actor with optional on_passivate satisfies behaviour" do
    assert <<>> == TestActorWithPassivate.on_passivate(<<>>)
  end
end
