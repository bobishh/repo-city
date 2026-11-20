defmodule RepoCityWeb.Endpoint do
  use Phoenix.Endpoint, otp_app: :repo_city

  plug(RepoCityWeb.Router)
end
