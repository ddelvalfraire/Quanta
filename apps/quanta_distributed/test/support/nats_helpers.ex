defmodule Quanta.Test.NatsHelpers do
  @moduledoc false

  def ensure_stream(stream_name, subjects) do
    {:ok, gnat} = Gnat.start_link(%{host: "localhost", port: 4222})

    payload =
      Jason.encode!(%{
        name: stream_name,
        subjects: [subjects],
        retention: "limits",
        storage: "memory",
        max_msgs: 1000
      })

    {:ok, %{body: _}} = Gnat.request(gnat, "$JS.API.STREAM.CREATE.#{stream_name}", payload)
    GenServer.stop(gnat)
  end

  def delete_stream(stream_name) do
    {:ok, gnat} = Gnat.start_link(%{host: "localhost", port: 4222})
    Gnat.request(gnat, "$JS.API.STREAM.DELETE.#{stream_name}", "")
    GenServer.stop(gnat)
  rescue
    _ -> :ok
  end

  def ensure_kv_bucket(bucket_name) do
    {:ok, gnat} = Gnat.start_link(%{host: "localhost", port: 4222})

    payload =
      Jason.encode!(%{
        name: "KV_#{bucket_name}",
        subjects: ["$KV.#{bucket_name}.>"],
        retention: "limits",
        storage: "memory",
        max_msgs_per_subject: 1,
        discard: "new",
        allow_rollup_hdrs: true,
        deny_delete: true,
        deny_purge: false,
        num_replicas: 1
      })

    {:ok, %{body: _}} = Gnat.request(gnat, "$JS.API.STREAM.CREATE.KV_#{bucket_name}", payload)
    GenServer.stop(gnat)
  end

  def delete_kv_bucket(bucket_name) do
    {:ok, gnat} = Gnat.start_link(%{host: "localhost", port: 4222})
    Gnat.request(gnat, "$JS.API.STREAM.DELETE.KV_#{bucket_name}", "")
    GenServer.stop(gnat)
  rescue
    _ -> :ok
  end
end
