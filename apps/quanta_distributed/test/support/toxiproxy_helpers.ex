defmodule Quanta.Test.ToxiproxyHelpers do
  @moduledoc """
  Setup/teardown helpers for Toxiproxy proxies to NATS.

  Assumes the Toxiproxy container from docker-compose.test.yml is running
  on localhost:8474, with proxies configured for 3 NATS nodes:

    - nats_1: 14222 -> nats-1:4222
    - nats_2: 14223 -> nats-2:4222
    - nats_3: 14224 -> nats-3:4222
  """

  @api_host "http://localhost:8474"

  @proxies %{
    nats_1: %{name: "nats_1", listen: "0.0.0.0:14222", upstream: "nats-1:4222"},
    nats_2: %{name: "nats_2", listen: "0.0.0.0:14223", upstream: "nats-2:4222"},
    nats_3: %{name: "nats_3", listen: "0.0.0.0:14224", upstream: "nats-3:4222"}
  }

  @doc """
  Create all NATS proxies in Toxiproxy. Idempotent.

  Call in `setup_all` for chaos tests that need proxy control.
  """
  @spec setup_proxies() :: :ok | {:error, term()}
  def setup_proxies do
    for {_key, proxy} <- @proxies do
      body = Jason.encode!(proxy)
      post("/proxies", body)
    end

    :ok
  end

  @doc """
  Remove all NATS proxies. Call in `on_exit` to clean up.
  """
  @spec teardown_proxies() :: :ok
  def teardown_proxies do
    for {_key, proxy} <- @proxies do
      delete("/proxies/#{proxy.name}")
    end

    :ok
  end

  @doc """
  Add a toxic to a proxy. Common toxics:

    - `"timeout"` — close connections after `timeout` ms
    - `"latency"` — add `latency` ms delay
    - `"bandwidth"` — limit to `rate` KB/s
    - `"down"` — (use `disable_proxy/1` instead)

  ## Examples

      add_toxic(:nats_1, "latency", %{latency: 500, jitter: 100})
      add_toxic(:nats_1, "timeout", %{timeout: 0})
  """
  @spec add_toxic(atom(), String.t(), map(), keyword()) :: {:ok, map()} | {:error, term()}
  def add_toxic(proxy_key, toxic_type, attributes, opts \\ []) do
    proxy = Map.fetch!(@proxies, proxy_key)
    stream = Keyword.get(opts, :stream, "downstream")
    toxicity = Keyword.get(opts, :toxicity, 1.0)

    body =
      Jason.encode!(%{
        name: "#{proxy.name}_#{toxic_type}",
        type: toxic_type,
        stream: stream,
        toxicity: toxicity,
        attributes: attributes
      })

    post("/proxies/#{proxy.name}/toxics", body)
  end

  @doc "Remove a specific toxic from a proxy."
  @spec remove_toxic(atom(), String.t()) :: :ok | {:error, term()}
  def remove_toxic(proxy_key, toxic_type) do
    proxy = Map.fetch!(@proxies, proxy_key)
    delete("/proxies/#{proxy.name}/toxics/#{proxy.name}_#{toxic_type}")
  end

  @doc "Disable a proxy entirely (simulates network partition to that NATS node)."
  @spec disable_proxy(atom()) :: :ok | {:error, term()}
  def disable_proxy(proxy_key) do
    proxy = Map.fetch!(@proxies, proxy_key)
    body = Jason.encode!(%{enabled: false})
    post_raw("/proxies/#{proxy.name}", body)
  end

  @doc "Re-enable a previously disabled proxy."
  @spec enable_proxy(atom()) :: :ok | {:error, term()}
  def enable_proxy(proxy_key) do
    proxy = Map.fetch!(@proxies, proxy_key)
    body = Jason.encode!(%{enabled: true})
    post_raw("/proxies/#{proxy.name}", body)
  end

  @doc "Reset all proxies — remove all toxics and re-enable everything."
  @spec reset_all() :: :ok
  def reset_all do
    post_raw("/reset", "")
    :ok
  end

  @doc "List all configured proxies from Toxiproxy API."
  @spec list_proxies() :: {:ok, map()} | {:error, term()}
  def list_proxies do
    get("/proxies")
  end

  # --- HTTP helpers (using :httpc from stdlib) ---

  defp post(path, body) do
    url = ~c"#{@api_host}#{path}"
    headers = [{~c"content-type", ~c"application/json"}]

    case :httpc.request(:post, {url, headers, ~c"application/json", body}, [], []) do
      {:ok, {{_, status, _}, _, resp_body}} when status in 200..299 ->
        {:ok, Jason.decode!(to_string(resp_body))}

      {:ok, {{_, 409, _}, _, _}} ->
        # Proxy already exists — idempotent
        :ok

      {:ok, {{_, status, _}, _, resp_body}} ->
        {:error, {status, to_string(resp_body)}}

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp post_raw(path, body) do
    url = ~c"#{@api_host}#{path}"
    headers = [{~c"content-type", ~c"application/json"}]

    case :httpc.request(:post, {url, headers, ~c"application/json", body}, [], []) do
      {:ok, {{_, status, _}, _, _}} when status in 200..299 -> :ok
      {:ok, {{_, status, _}, _, resp_body}} -> {:error, {status, to_string(resp_body)}}
      {:error, reason} -> {:error, reason}
    end
  end

  defp get(path) do
    url = ~c"#{@api_host}#{path}"

    case :httpc.request(:get, {url, []}, [], []) do
      {:ok, {{_, 200, _}, _, resp_body}} ->
        {:ok, Jason.decode!(to_string(resp_body))}

      {:ok, {{_, status, _}, _, resp_body}} ->
        {:error, {status, to_string(resp_body)}}

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp delete(path) do
    url = ~c"#{@api_host}#{path}"

    case :httpc.request(:delete, {url, []}, [], []) do
      {:ok, {{_, status, _}, _, _}} when status in 200..299 -> :ok
      {:ok, {{_, 404, _}, _, _}} -> :ok
      {:ok, {{_, status, _}, _, resp_body}} -> {:error, {status, to_string(resp_body)}}
      {:error, reason} -> {:error, reason}
    end
  end
end
