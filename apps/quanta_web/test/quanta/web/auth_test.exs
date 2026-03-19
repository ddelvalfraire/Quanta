defmodule Quanta.Web.AuthTest do
  use ExUnit.Case, async: true

  alias Quanta.Web.Auth

  @admin_key "qk_admin_test_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  @rw_key "qk_rw_test_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  @ro_key "qk_ro_test_cccccccccccccccccccccccccccccccc"

  describe "authenticate/1" do
    test "returns scope and namespace for valid admin key" do
      assert {:ok, :admin, "test"} = Auth.authenticate(@admin_key)
    end

    test "returns scope and namespace for valid rw key" do
      assert {:ok, :rw, "test"} = Auth.authenticate(@rw_key)
    end

    test "returns scope and namespace for valid ro key" do
      assert {:ok, :ro, "test"} = Auth.authenticate(@ro_key)
    end

    test "returns :error for invalid key format" do
      assert :error = Auth.authenticate("bad-key-format")
    end

    test "returns :error for wrong key value" do
      assert :error = Auth.authenticate("qk_admin_test_zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz")
    end

    test "returns :error for empty string" do
      assert :error = Auth.authenticate("")
    end
  end
end
