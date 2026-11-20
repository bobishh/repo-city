FROM elixir:1.20.5-otp-29-slim AS base
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

FROM base AS release
WORKDIR /build
ENV MIX_ENV=prod
RUN mix local.hex --force && mix local.rebar --force
COPY server/mix.exs server/mix.lock ./
COPY server/config/ config/
RUN mix deps.get --only prod --check-locked && mix deps.compile
COPY server/lib/ lib/
RUN mix compile --warnings-as-errors && mix release
COPY web/dist/ static/
RUN test -s static/index.html && test -s static/game/repo_city.js && \
    test -s static/game/repo_city_bg.wasm.gz && gzip -dfk static/game/repo_city_bg.wasm.gz

FROM base
WORKDIR /app
LABEL org.opencontainers.image.source="https://github.com/bobishh/repo-city" service="repo-city"
ARG REPO_CITY_REVISION
ENV PHX_SERVER=true PORT=8080 STATIC_ROOT=/app/static REPO_CITY_REVISION=$REPO_CITY_REVISION
COPY --from=release /build/_build/prod/rel/repo_city/ ./
COPY --from=release /build/static/ static/
USER 65534:65534
EXPOSE 8080
CMD ["/app/bin/repo_city", "start"]
