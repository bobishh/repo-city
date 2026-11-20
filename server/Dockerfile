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
RUN engine=$(sed -n 's/.*data-engine-build="\([0-9a-f]*\)".*/\1/p' static/index.html) && \
    test "${#engine}" -eq 24 && test -s "static/game/$engine/repo_city.js" && \
    gzip -dfk "static/game/$engine/repo_city_bg.wasm.gz" && \
    ln -sf "$engine/repo_city_bg.wasm" static/game/repo_city_bg.wasm

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
