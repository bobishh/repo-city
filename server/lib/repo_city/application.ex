defmodule RepoCity.Application do
  use Application

  @impl true
  def start(_type, _args) do
    [
      {Task.Supervisor, name: RepoCity.GitHub.Tasks},
      RepoCity.GitHub.PublicCache,
      RepoCityWeb.Endpoint
    ]
    |> Supervisor.start_link(strategy: :one_for_one, name: RepoCity.Supervisor)
  end
end
