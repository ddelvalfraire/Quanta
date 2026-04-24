defmodule Quanta.Web.Auth do
  @moduledoc false

  @key_pattern ~r/^qk_(admin|rw|ro)_([a-zA-Z0-9_-]+)_([a-zA-Z0-9]{32})$/

  @spec authenticate(String.t()) :: {:ok, atom(), String.t()} | :error
  def authenticate(token) do
    with :ok <- validate_key(token),
         {:ok, scope, namespace} <- parse_key(token) do
      {:ok, scope, namespace}
    end
  end

  defp parse_key(token) do
    case Regex.run(@key_pattern, token) do
      [_, scope_str, namespace, _random] ->
        {:ok, scope_atom(scope_str), namespace}

      _ ->
        :error
    end
  end

  defp scope_atom("admin"), do: :admin
  defp scope_atom("rw"), do: :rw
  defp scope_atom("ro"), do: :ro

  defp validate_key(token) do
    api_keys = Application.get_env(:quanta_web, :api_keys, [])

    if Enum.any?(api_keys, &Plug.Crypto.secure_compare(&1, token)) do
      :ok
    else
      :error
    end
  end
end
