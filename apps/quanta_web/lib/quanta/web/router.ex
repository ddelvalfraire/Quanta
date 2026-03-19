defmodule Quanta.Web.Router do
  use Phoenix.Router

  pipeline :api do
    plug :accepts, ["json"]
  end

  pipeline :authenticated do
    plug Quanta.Web.Plugs.Auth
    plug Quanta.Web.Plugs.RequireNamespace
  end

  # Health routes bypass :api pipeline — probes must not be rejected on Accept negotiation
  scope "/health", Quanta.Web do
    get "/live", HealthController, :live
    get "/ready", HealthController, :ready
  end

  scope "/api/internal", Quanta.Web do
    get "/drain", DrainController, :drain
  end

  scope "/api/v1", Quanta.Web do
    pipe_through [:api, :authenticated]

    scope "/actors/:ns/:type" do
      post "/", ActorController, :spawn
      post "/:id/messages", ActorController, :send_message
      get "/:id/state", ActorController, :get_state
      get "/:id/meta", ActorController, :get_meta
      delete "/:id", ActorController, :destroy
    end

    scope "/types/:ns" do
      get "/", TypeController, :list_types
      post "/:type/deploy", TypeController, :deploy
    end
  end
end
