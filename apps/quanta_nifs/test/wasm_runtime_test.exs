defmodule Quanta.Nifs.WasmRuntimeTest do
  use ExUnit.Case, async: true

  alias Quanta.Nifs.WasmRuntime
  alias Quanta.Nifs.EffectDecoder

  @fixture_path Path.join([__DIR__, "fixtures", "counter_actor.wasm"])
  @hmac_key Application.compile_env!(:quanta_nifs, :wasm_hmac_key)
  @fuel Application.compile_env!(:quanta_nifs, :default_fuel_limit)
  @mem Application.compile_env!(:quanta_nifs, :default_memory_limit_bytes)

  setup_all do
    wasm_bytes = File.read!(@fixture_path)
    {:ok, engine} = WasmRuntime.engine_new()
    {:ok, component} = WasmRuntime.component_compile(engine, wasm_bytes)
    {:ok, linker} = WasmRuntime.linker_new(engine)

    %{engine: engine, component: component, linker: linker, wasm_bytes: wasm_bytes}
  end

  # 1. engine_new creates engine
  test "engine_new creates engine", _ctx do
    assert {:ok, engine} = WasmRuntime.engine_new()
    assert is_reference(engine)
  end

  # 2. component_compile with valid WASM succeeds
  test "component_compile with valid WASM succeeds", %{engine: engine, wasm_bytes: wasm_bytes} do
    assert {:ok, component} = WasmRuntime.component_compile(engine, wasm_bytes)
    assert is_reference(component)
  end

  # 3. component_serialize + component_deserialize roundtrip
  test "serialize/deserialize roundtrip", %{engine: engine, component: component, linker: linker} do
    {:ok, serialized} = WasmRuntime.component_serialize(component, @hmac_key)
    assert is_binary(serialized)
    assert byte_size(serialized) > 32

    {:ok, deserialized} = WasmRuntime.component_deserialize(engine, serialized, @hmac_key)
    assert is_reference(deserialized)

    # Verify the deserialized component works
    {:ok, state, effects} = WasmRuntime.call_init(engine, deserialized, linker, <<>>, @fuel, @mem)
    assert byte_size(state) == 8
    assert effects == []
  end

  # 4. Wrong HMAC → {:error, :hmac_mismatch}
  test "wrong HMAC key returns hmac_mismatch", %{engine: engine, component: component} do
    {:ok, serialized} = WasmRuntime.component_serialize(component, @hmac_key)
    wrong_key = :crypto.strong_rand_bytes(32)
    assert {:error, :hmac_mismatch} = WasmRuntime.component_deserialize(engine, serialized, wrong_key)
  end

  # 5. call_init → 8-byte zero state + empty effects
  test "call_init returns zero state and empty effects", %{engine: engine, component: component, linker: linker} do
    {:ok, state, effects} = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, @mem)
    assert state == <<0, 0, 0, 0, 0, 0, 0, 0>>
    assert effects == []
  end

  # 6. call_handle_message("inc") → counter=1, persist effect
  test "handle_message inc increments counter", %{engine: engine, component: component, linker: linker} do
    {:ok, state, _} = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, @mem)

    envelope = %{"source" => "test", "payload" => "inc"}
    {:ok, new_state, effects} = WasmRuntime.call_handle_message(engine, component, linker, state, envelope, @fuel, @mem)

    counter = :binary.decode_unsigned(new_state, :little)
    assert counter == 1
    assert length(effects) == 1
    assert [%{"type" => "persist"}] = effects
  end

  # 7. call_handle_message("get") → reply with counter bytes
  test "handle_message get returns reply effect", %{engine: engine, component: component, linker: linker} do
    {:ok, state, _} = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, @mem)

    # Increment first
    envelope_inc = %{"source" => "test", "payload" => "inc"}
    {:ok, state, _} = WasmRuntime.call_handle_message(engine, component, linker, state, envelope_inc, @fuel, @mem)

    # Get
    envelope_get = %{"source" => "test", "payload" => "get"}
    {:ok, _state, effects} = WasmRuntime.call_handle_message(engine, component, linker, state, envelope_get, @fuel, @mem)

    assert [%{"type" => "reply", "data" => reply_data}] = effects
    counter = :binary.decode_unsigned(reply_data, :little)
    assert counter == 1
  end

  # 8. call_handle_timer → counter+=10, persist effect
  test "handle_timer increments counter by 10", %{engine: engine, component: component, linker: linker} do
    {:ok, state, _} = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, @mem)

    {:ok, new_state, effects} = WasmRuntime.call_handle_timer(engine, component, linker, state, "tick", @fuel, @mem)

    counter = :binary.decode_unsigned(new_state, :little)
    assert counter == 10
    assert [%{"type" => "persist"}] = effects
  end

  # 9. call_migrate → {:error, :not_exported}
  test "call_migrate returns not_exported for counter actor", %{engine: engine, component: component, linker: linker} do
    {:ok, state, _} = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, @mem)

    assert {:error, :not_exported} = WasmRuntime.call_migrate(engine, component, linker, state, 1, @fuel, @mem)
  end

  # 10. call_on_passivate → returns state as-is
  test "on_passivate returns state unchanged", %{engine: engine, component: component, linker: linker} do
    {:ok, state, _} = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, @mem)

    # Increment to get non-zero state
    envelope = %{"source" => "test", "payload" => "inc"}
    {:ok, state, _} = WasmRuntime.call_handle_message(engine, component, linker, state, envelope, @fuel, @mem)

    {:ok, passivated_state} = WasmRuntime.call_on_passivate(engine, component, linker, state, @fuel, @mem)
    assert passivated_state == state
  end

  # 11. Fuel exhaustion (fuel_limit=1) → {:error, :fuel_exhausted}
  test "fuel exhaustion returns fuel_exhausted", %{engine: engine, component: component, linker: linker} do
    assert {:error, :fuel_exhausted} = WasmRuntime.call_init(engine, component, linker, <<>>, 1, @mem)
  end

  # 12. Memory exceeded → {:error, :memory_exceeded}
  test "memory limit exceeded returns memory_exceeded", %{engine: engine, component: component, linker: linker} do
    # Use an extremely small memory limit (1 byte) — component instantiation should fail
    result = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, 1)
    assert {:error, :memory_exceeded} = result
  end

  # 13. Invalid WASM → compile error
  test "component_compile with invalid WASM returns error", %{engine: engine} do
    assert {:error, _reason} = WasmRuntime.component_compile(engine, "not valid wasm")
  end

  # 14. Effects decoded correctly as maps
  test "effects decoded correctly with EffectDecoder", %{engine: engine, component: component, linker: linker} do
    {:ok, state, _} = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, @mem)

    # Inc → persist effect
    envelope = %{"source" => "test", "payload" => "inc"}
    {:ok, state, effects} = WasmRuntime.call_handle_message(engine, component, linker, state, envelope, @fuel, @mem)

    decoded = EffectDecoder.decode_all(effects)
    assert decoded == [:persist]

    # Get → reply effect
    envelope_get = %{"source" => "test", "payload" => "get"}
    {:ok, _state, effects} = WasmRuntime.call_handle_message(engine, component, linker, state, envelope_get, @fuel, @mem)

    decoded = EffectDecoder.decode_all(effects)
    assert [{:reply, reply_data}] = decoded
    assert :binary.decode_unsigned(reply_data, :little) == 1
  end

  # 15. component_compile with garbage bytes → {:error, _}
  test "component_compile with garbage bytes returns error", %{engine: engine} do
    garbage = :crypto.strong_rand_bytes(64)
    assert {:error, _reason} = WasmRuntime.component_compile(engine, garbage)
  end

  # 16. call_handle_message("log") → log effect with message
  test "handle_message log returns log effect", %{engine: engine, component: component, linker: linker} do
    {:ok, state, _} = WasmRuntime.call_init(engine, component, linker, <<>>, @fuel, @mem)

    envelope = %{"source" => "test", "payload" => "log"}
    {:ok, new_state, effects} = WasmRuntime.call_handle_message(engine, component, linker, state, envelope, @fuel, @mem)

    # State unchanged (log doesn't modify counter)
    assert new_state == state

    # Log effect returned
    assert [%{"type" => "log", "message" => "hello from actor"}] = effects

    # Verify decoder handles it
    decoded = EffectDecoder.decode_all(effects)
    assert [{:log, "hello from actor"}] = decoded
  end
end
