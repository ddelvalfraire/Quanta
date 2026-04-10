defmodule Quanta.Web.Actors.FileActor do
  @behaviour Quanta.Actor

  @impl true
  def init(_payload) do
    {<<>>, []}
  end

  @impl true
  def handle_message(state, envelope) do
    case Jason.decode(envelope.payload) do
      {:ok, %{"type" => "run", "code" => code}} when is_binary(code) ->
        case Quanta.Wasm.JsExecutor.execute(code) do
          {:ok, stdout, stderr} ->
            {state, [{:broadcast_output, %{status: "ok", stdout: stdout, stderr: stderr}}]}

          {:error, reason} ->
            {state, [{:broadcast_output, %{status: "error", error: to_string(reason)}}]}
        end

      _ ->
        {state, []}
    end
  end

  @impl true
  def handle_timer(state, _name), do: {state, []}

  @impl true
  def on_passivate(state), do: state
end
