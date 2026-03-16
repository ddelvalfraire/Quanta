defmodule Quanta.Web.Router do
  use Phoenix.Router

  pipeline :api do
    plug :accepts, ["json"]
  end

  # Health routes bypass :api pipeline — probes must not be rejected on Accept negotiation
  scope "/health", Quanta.Web do
    get "/live", HealthController, :live
    get "/ready", HealthController, :ready
  end
end
