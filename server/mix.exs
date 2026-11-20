defmodule RepoCity.MixProject do
  use Mix.Project

  def project do
    [
      app: :repo_city,
      version: "0.1.0",
      elixir: "~> 1.18",
      start_permanent: Mix.env() == :prod,
      deps: [
        {:phoenix, "~> 1.8"},
        {:bandit, "~> 1.10"},
        {:req, "~> 0.7"},
        {:jason, "~> 1.4"},
        {:plug, "~> 1.19"},
        {:websock_adapter, "~> 0.7"}
      ]
    ]
  end

  def application do
    [mod: {RepoCity.Application, []}, extra_applications: [:logger, :crypto]]
  end
end
