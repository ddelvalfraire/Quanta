defmodule Quanta.Nats.CoreSupervisor do
  @moduledoc """
  Starts a pool of `Gnat.ConnectionSupervisor` children, each registered as
  `:"quanta_nats_0"`, `:"quanta_nats_1"`, etc.

  Reads `:nats_urls` and `:nats_pool_size` from the `:quanta_distributed` app env.
  Each connection supervisor gets the full list of URLs as failover targets.
  """

  use Supervisor

  @default_pool_size 2

  def start_link(opts \\ []) do
    Supervisor.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @impl true
  def init(_opts) do
    urls = Application.get_env(:quanta_distributed, :nats_urls, ["nats://localhost:4222"])
    pool_size = Application.get_env(:quanta_distributed, :nats_pool_size, @default_pool_size)

    connection_settings = Enum.map(urls, &parse_nats_url/1)

    children =
      for i <- 0..(pool_size - 1) do
        name = :"quanta_nats_#{i}"

        settings = %{
          name: name,
          connection_settings: connection_settings
        }

        Supervisor.child_spec({Gnat.ConnectionSupervisor, settings}, id: name)
      end

    Supervisor.init(children, strategy: :one_for_one)
  end

  @doc """
  Parse a NATS URL string into a connection settings map for Gnat.

  Accepts `nats://host:port`, `host:port`, or `host` (port defaults to 4222).
  """
  @spec parse_nats_url(String.t()) :: %{host: charlist(), port: non_neg_integer()}
  def parse_nats_url(url) do
    uri =
      url
      |> ensure_scheme()
      |> URI.parse()

    %{
      host: to_charlist(uri.host),
      port: uri.port || 4222
    }
  end

  defp ensure_scheme(url) do
    if String.contains?(url, "://"), do: url, else: "nats://#{url}"
  end
end
