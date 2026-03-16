defmodule Quanta.Nifs.Native do
  @moduledoc """
  Native Rust NIF bindings loaded via Rustler.
  """

  use Rustler,
    otp_app: :quanta_nifs,
    crate: "quanta_nifs",
    path: "../../rust/quanta-nifs"

  @doc "Smoke test: returns true if the NIF is loaded."
  @spec ping() :: boolean()
  def ping(), do: :erlang.nif_error(:nif_not_loaded)
end
