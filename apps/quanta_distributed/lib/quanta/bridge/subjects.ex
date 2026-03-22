defmodule Quanta.Bridge.Subjects do
  @moduledoc """
  NATS subject builders for the bridge protocol.

  Conventions:
  - `d2r` = distributed-to-realtime (Elixir -> Rust server)
  - `r2d` = realtime-to-distributed (Rust server -> Elixir)
  """

  @doc "Specific d2r subject for a given island, type, and id."
  @spec d2r(String.t(), String.t(), String.t(), String.t()) ::
          {:ok, String.t()} | {:error, :invalid_subject_segment}
  def d2r(ns, island_id, type, id) do
    with :ok <- validate_segments([ns, island_id, type, id]) do
      {:ok, "quanta.#{ns}.bridge.d2r.#{island_id}.#{type}.#{id}"}
    end
  end

  @doc "Wildcard subscription for all d2r messages on a specific island."
  @spec d2r_wildcard(String.t(), String.t()) ::
          {:ok, String.t()} | {:error, :invalid_subject_segment}
  def d2r_wildcard(ns, island_id) do
    with :ok <- validate_segments([ns, island_id]) do
      {:ok, "quanta.#{ns}.bridge.d2r.#{island_id}.>"}
    end
  end

  @doc "Catch-all subscription for all d2r messages across all islands."
  @spec d2r_catch_all(String.t()) :: {:ok, String.t()} | {:error, :invalid_subject_segment}
  def d2r_catch_all(ns) do
    with :ok <- validate_segments([ns]) do
      {:ok, "quanta.#{ns}.bridge.d2r.>"}
    end
  end

  @doc "Queue group name for d2r subscriptions."
  @spec d2r_queue_group() :: String.t()
  def d2r_queue_group, do: "quanta-bridge-d2r"

  @doc "Specific r2d subject for a given type and id."
  @spec r2d(String.t(), String.t(), String.t()) ::
          {:ok, String.t()} | {:error, :invalid_subject_segment}
  def r2d(ns, type, id) do
    with :ok <- validate_segments([ns, type, id]) do
      {:ok, "quanta.#{ns}.bridge.r2d.#{type}.#{id}"}
    end
  end

  @doc "Wildcard subscription for all r2d messages."
  @spec r2d_wildcard(String.t()) :: {:ok, String.t()} | {:error, :invalid_subject_segment}
  def r2d_wildcard(ns) do
    with :ok <- validate_segments([ns]) do
      {:ok, "quanta.#{ns}.bridge.r2d.>"}
    end
  end

  @doc "Queue group name for r2d subscriptions."
  @spec r2d_queue_group() :: String.t()
  def r2d_queue_group, do: "quanta-bridge-r2d"

  @doc "Parse a d2r subject into its component parts."
  @spec parse_d2r(String.t()) :: {:ok, map()} | {:error, String.t()}
  def parse_d2r(subject) do
    case String.split(subject, ".") do
      ["quanta", ns, "bridge", "d2r", island_id, type, id] ->
        {:ok, %{ns: ns, island_id: island_id, type: type, id: id}}

      _ ->
        {:error, "subject does not match quanta.{ns}.bridge.d2r.{island_id}.{type}.{id}"}
    end
  end

  defp validate_segments(segments) do
    if Enum.all?(segments, &valid_segment?/1), do: :ok, else: {:error, :invalid_subject_segment}
  end

  defp valid_segment?(s) when is_binary(s) do
    s != "" and not String.contains?(s, [".", "*", ">"])
  end
end
