defmodule Quanta.ULIDTest do
  use ExUnit.Case, async: true

  alias Quanta.ULID

  @crockford_chars String.to_charlist("0123456789ABCDEFGHJKMNPQRSTVWXYZ")

  setup do
    Process.delete({Quanta.ULID, :state})
    :ok
  end

  describe "generate/0" do
    test "returns a 26-character string" do
      assert String.length(ULID.generate()) == 26
    end

    test "uses only valid Crockford base32 characters" do
      ulid = ULID.generate()

      for <<char <- ulid>> do
        assert char in @crockford_chars, "invalid char: #{<<char>>}"
      end
    end

    test "sequential calls are monotonically ordered" do
      ulids = for _ <- 1..100, do: ULID.generate()
      assert ulids == Enum.sort(ulids)
    end
  end

  describe "generate/1" do
    test "different timestamps produce different prefixes" do
      a = ULID.generate(1000)
      Process.delete({Quanta.ULID, :state})
      b = ULID.generate(2000)
      assert String.slice(a, 0, 10) != String.slice(b, 0, 10)
    end

    test "later timestamp produces lexicographically greater prefix" do
      a = ULID.generate(1000)
      Process.delete({Quanta.ULID, :state})
      b = ULID.generate(2000)
      assert String.slice(a, 0, 10) < String.slice(b, 0, 10)
    end

    test "same timestamp produces monotonically increasing values" do
      ulids = for _ <- 1..100, do: ULID.generate(1_700_000_000_000)
      assert ulids == Enum.sort(ulids)
    end

    test "same timestamp preserves the timestamp prefix" do
      ulids = for _ <- 1..10, do: ULID.generate(1_700_000_000_000)
      prefixes = Enum.map(ulids, &String.slice(&1, 0, 10))
      assert length(Enum.uniq(prefixes)) == 1
    end
  end
end
