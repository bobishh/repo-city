defmodule RepoCity.Application do
  use Application

  @impl true
  def start(_type, _args) do
    [
      {Task.Supervisor, name: RepoCity.GitHub.Tasks},
      RepoCity.GitHub.PublicCache,
      {Registry, keys: :unique, name: RepoCity.ReviewParty.Registry},
      {DynamicSupervisor,
       strategy: :one_for_one,
       name: RepoCity.ReviewParty.Rooms,
       max_children: Application.get_env(:repo_city, :party_active_rooms, 256)},
      RepoCityWeb.Endpoint
    ]
    |> Supervisor.start_link(strategy: :one_for_one, name: RepoCity.Supervisor)
  end
end
