defmodule Quanta.Web.Test.GameActor do
  @moduledoc false
  @behaviour Quanta.Actor

  @game_state_wit """
  record game-state {
      is-alive: bool,
      /// @quanta:clamp(0, 100)
      health: u16,
      score: s32,
  }
  """

  def wit_source, do: @game_state_wit

  @impl true
  def init(_payload) do
    {:ok, schema_ref, _} =
      Quanta.Nifs.SchemaCompiler.compile(@game_state_wit, "game-state")

    {:ok, state} = Quanta.Nifs.DeltaEncoder.encode_state(schema_ref, [true, 100, 0])
    {state, [{:persist, state}]}
  end

  @impl true
  def handle_message(state, envelope) do
    schema_ref = schema_ref()
    {:ok, decoded} = Quanta.Nifs.DeltaEncoder.decode_state(schema_ref, state)

    case envelope.payload do
      "take_damage" ->
        new_health = max(decoded["health"] - 10, 0)

        {:ok, new_state} =
          Quanta.Nifs.DeltaEncoder.encode_state(
            schema_ref,
            [decoded["is-alive"], new_health, decoded["score"]]
          )

        {new_state, [{:persist, new_state}, {:reply, new_state}]}

      "add_score" ->
        new_score = decoded["score"] + 100

        {:ok, new_state} =
          Quanta.Nifs.DeltaEncoder.encode_state(
            schema_ref,
            [decoded["is-alive"], decoded["health"], new_score]
          )

        {new_state, [{:persist, new_state}, {:reply, new_state}]}

      "get" ->
        {state, [{:reply, state}]}

      _ ->
        {state, []}
    end
  end

  @impl true
  def handle_timer(state, _), do: {state, []}

  defp schema_ref do
    case :persistent_term.get({__MODULE__, :schema_ref}, nil) do
      nil ->
        {:ok, ref, _} = Quanta.Nifs.SchemaCompiler.compile(@game_state_wit, "game-state")
        :persistent_term.put({__MODULE__, :schema_ref}, ref)
        ref

      ref ->
        ref
    end
  end
end

defmodule Quanta.Web.ActorChannelDeltaTest do
  use Quanta.Web.ChannelCase, async: false

  alias Quanta.Nifs.SchemaCompiler

  setup do
    wit = Quanta.Web.Test.GameActor.wit_source()
    {:ok, schema_ref, _warnings} = SchemaCompiler.compile(wit, "game-state")
    {:ok, schema_bytes} = SchemaCompiler.export(schema_ref)

    :ok =
      Quanta.Actor.ManifestRegistry.put(%Quanta.Manifest{
        version: "1",
        type: "game",
        namespace: "test",
        state: %Quanta.Manifest.State{kind: {:schematized, "game-state"}, version: 1}
      })

    Quanta.Actor.SchemaEvolution.put_cached_schema("test", "game", schema_bytes)

    prev = Application.get_env(:quanta_distributed, :actor_modules, %{})
    modules = Map.put(prev, {"test", "game"}, Quanta.Web.Test.GameActor)
    Application.put_env(:quanta_distributed, :actor_modules, modules)

    on_exit(fn ->
      Application.put_env(:quanta_distributed, :actor_modules, prev)
    end)

    %{schema_ref: schema_ref, schema_bytes: schema_bytes}
  end

  describe "join with format selection" do
    test "binary format includes schema bytes and base64 state", %{schema_bytes: schema_bytes} do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, reply, _socket} =
               subscribe_and_join(socket, "actor:test:game:bin-1", %{"format" => "binary"})

      assert %{schema: schema_b64, state: state_b64, schema_version: 1, seq: 0} = reply
      assert {:ok, ^schema_bytes} = Base.decode64(schema_b64)
      assert {:ok, _state} = Base.decode64(state_b64)
    end

    test "json format includes decoded state object" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, reply, _socket} =
               subscribe_and_join(socket, "actor:test:game:json-1", %{"format" => "json"})

      assert %{state: state, schema_version: 1, seq: 0} = reply
      assert is_map(state)
      assert state["is-alive"] == true
      assert state["health"] == 100
      assert state["score"] == 0
    end

    test "default format is binary with schema when schematized" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, reply, _socket} =
               subscribe_and_join(socket, "actor:test:game:default-1", %{})

      assert Map.has_key?(reply, :state)
    end

    test "invalid format rejected" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:error, %{reason: "invalid_format"}} =
               subscribe_and_join(socket, "actor:test:game:bad-1", %{"format" => "xml"})
    end
  end

  describe "delta push on state change" do
    test "binary delta pushed after state change" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "actor:test:game:delta-bin-1", %{"format" => "binary"})

      assert_push "presence_state", _

      ref = push(socket, "message", %{"payload" => Base.encode64("take_damage")})
      assert_reply ref, :ok, _

      assert_push "delta", %{delta: delta_b64, seq: 1}, 1_000
      assert {:ok, delta} = Base.decode64(delta_b64)
      assert byte_size(delta) > 0
    end

    test "json delta pushed with changed field names" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, reply, socket} =
        subscribe_and_join(socket, "actor:test:game:delta-json-1", %{"format" => "json"})

      assert Map.has_key?(reply, :schema_version)
      assert_push "presence_state", _

      ref = push(socket, "message", %{"payload" => Base.encode64("take_damage")})
      assert_reply ref, :ok, _

      assert_push "delta", %{state: state, changed: changed, seq: 1}, 1_000
      assert is_map(state)
      assert is_list(changed)
      assert "health" in changed
    end
  end

  describe "prediction mode" do
    test "non-prediction actor rejects input" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "actor:test:game:pred-1", %{"format" => "binary"})

      ref = push(socket, "input", %{"input_seq" => 1, "data" => Base.encode64("move")})
      assert_reply ref, :error, %{reason: "prediction_not_enabled"}
    end

    test "ro scope cannot send input" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @ro_key})

      {:ok, _reply, socket} =
        subscribe_and_join(socket, "actor:test:game:pred-2", %{"format" => "binary"})

      ref = push(socket, "input", %{"input_seq" => 1})
      assert_reply ref, :error, %{reason: "insufficient_scope"}
    end
  end

  describe "opaque actor fallback" do
    test "opaque actor join still returns base64 state" do
      {:ok, socket} = connect(Quanta.Web.ActorSocket, %{"token" => @rw_key})

      assert {:ok, reply, _socket} =
               subscribe_and_join(socket, "actor:test:counter:opaque-1", %{"format" => "binary"})

      assert %{state: state_b64} = reply
      assert {:ok, _} = Base.decode64(state_b64)
      refute Map.has_key?(reply, :schema)
    end
  end
end
