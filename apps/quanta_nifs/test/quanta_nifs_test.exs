defmodule QuantaNifsTest do
  use ExUnit.Case

  test "NIF is loaded" do
    assert Quanta.Nifs.Native.ping() == true
  end
end
