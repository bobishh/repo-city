FROM node:22-alpine
WORKDIR /app
LABEL org.opencontainers.image.source="https://github.com/bobishh/repo-city" service="repo-city"
ARG REPO_CITY_REVISION
ENV NODE_ENV=production PORT=8080 REPO_CITY_REVISION=$REPO_CITY_REVISION
COPY server.mjs github-auth.mjs github-public.mjs pr-review.mjs ./
COPY dist/ ./dist/
RUN test -s dist/index.html && test -s dist/game/repo_city.js && \
    (test -s dist/game/repo_city_bg.wasm.gz || test -s dist/game/repo_city_bg.wasm)
USER node
EXPOSE 8080
CMD ["node", "server.mjs"]
