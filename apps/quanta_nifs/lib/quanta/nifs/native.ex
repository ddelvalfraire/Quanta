defmodule Quanta.Nifs.Native do
  @moduledoc """
  Native Rust NIF bindings loaded via Rustler.
  """

  use Rustler,
    otp_app: :quanta_nifs,
    crate: "quanta_nifs",
    path: "../../rust/quanta-nifs"

  @spec ping() :: boolean()
  def ping(), do: :erlang.nif_error(:nif_not_loaded)

  @spec encode_envelope_header(map()) :: {:ok, binary()} | {:error, String.t()}
  def encode_envelope_header(_header), do: :erlang.nif_error(:nif_not_loaded)

  @spec decode_envelope_header(binary()) :: {:ok, map()} | {:error, String.t()}
  def decode_envelope_header(_binary), do: :erlang.nif_error(:nif_not_loaded)
end
