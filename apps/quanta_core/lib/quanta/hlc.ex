defmodule Quanta.HLC do
  @moduledoc """
  Hybrid Logical Clock — a timestamp combining wall-clock time with a logical counter.

  Wire format: 10 bytes — `<<wall::unsigned-big-64, logical::unsigned-big-16>>`.
  """

  @enforce_keys [:wall, :logical]
  defstruct [:wall, :logical]

  @type t :: %__MODULE__{
          wall: integer(),
          logical: 0..65535
        }

  @spec now() :: t()
  def now do
    %__MODULE__{wall: System.os_time(:microsecond), logical: 0}
  end

  @spec compare(t(), t()) :: :lt | :eq | :gt
  def compare(%__MODULE__{} = a, %__MODULE__{} = b) do
    a_key = {a.wall, a.logical}
    b_key = {b.wall, b.logical}

    cond do
      a_key < b_key -> :lt
      a_key > b_key -> :gt
      true -> :eq
    end
  end

  @spec encode(t()) :: <<_::80>>
  def encode(%__MODULE__{wall: wall, logical: logical}) do
    <<wall::unsigned-big-64, logical::unsigned-big-16>>
  end

  @spec decode(<<_::80>>) :: t()
  def decode(<<wall::unsigned-big-64, logical::unsigned-big-16>>) do
    %__MODULE__{wall: wall, logical: logical}
  end
end
