defmodule Quanta.Nifs.JsExecutorTest do
  use ExUnit.Case, async: true

  alias Quanta.Nifs.JsExecutor

  @wasm_path Application.app_dir(:quanta_nifs, "priv/wasm/javy_quickjs_provider.wasm")
  @fuel 1_000_000
  @memory 16_777_216

  setup_all do
    wasm_bytes = File.read!(@wasm_path)
    %{wasm: wasm_bytes}
  end

  test "console.log produces stdout", %{wasm: wasm} do
    assert {:ok, stdout, stderr} =
             JsExecutor.execute(wasm, ~s[console.log("hello")], @fuel, @memory)

    assert stdout =~ "hello"
    assert stderr == "" || stderr == nil
  end

  test "multiple console.log lines", %{wasm: wasm} do
    code = ~s[console.log("one"); console.log("two")]

    assert {:ok, stdout, _stderr} = JsExecutor.execute(wasm, code, @fuel, @memory)
    assert stdout =~ "one"
    assert stdout =~ "two"
  end

  test "console.error writes to stderr", %{wasm: wasm} do
    assert {:ok, _stdout, stderr} =
             JsExecutor.execute(wasm, ~s[console.error("oops")], @fuel, @memory)

    assert stderr =~ "oops"
  end

  test "empty code produces no output", %{wasm: wasm} do
    assert {:ok, stdout, stderr} = JsExecutor.execute(wasm, "", @fuel, @memory)
    assert stdout == ""
    assert stderr == "" || stderr == nil
  end

  test "syntax error returns error", %{wasm: wasm} do
    result = JsExecutor.execute(wasm, "}{invalid", @fuel, @memory)

    case result do
      {:error, _reason} -> :ok
      {:ok, _stdout, stderr} -> assert stderr =~ "Syntax" or stderr != ""
    end
  end

  test "fuel exhaustion on infinite loop", %{wasm: wasm} do
    result = JsExecutor.execute(wasm, "while(true){}", @fuel, @memory)
    assert {:error, :fuel_exhausted} = result
  end

  test "basic arithmetic", %{wasm: wasm} do
    assert {:ok, stdout, _} =
             JsExecutor.execute(wasm, ~s[console.log(2 + 3)], @fuel, @memory)

    assert stdout =~ "5"
  end
end
