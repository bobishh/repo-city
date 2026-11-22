defmodule RepoCity.Application do
  use Application

  @impl true
  def start(_type, _args) do
    deployment_index =
      RepoCityWeb.Static.deployment_index!(
        Application.get_env(:repo_city, :static_root),
        Application.get_env(:repo_city, :browser_script)
      )

    Application.put_env(:repo_city, :deployment_index, deployment_index)

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
