defmodule Quanta.Web.ChannelCase do
  @moduledoc false
  use ExUnit.CaseTemplate

  using do
    quote do
      import Phoenix.ChannelTest

      @endpoint Quanta.Web.Endpoint

      @admin_key "qk_admin_test_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      @rw_key "qk_rw_test_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      @ro_key "qk_ro_test_cccccccccccccccccccccccccccccccc"
    end
  end

  setup do
    {:ok, prev_modules: prev} = Quanta.Web.TestSetup.reset_actor_environment()
    on_exit(fn -> Application.put_env(:quanta_distributed, :actor_modules, prev) end)
    :ok
  end
end
