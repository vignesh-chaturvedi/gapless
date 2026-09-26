# One image: gapless-server, the built console and the offline fixture.
#   docker compose up          (or: docker build -t gapless . && docker run -p 8790:8790 gapless)

# ── console ─────────────────────────────────────────────────────────────
FROM node:24-slim AS console
WORKDIR /src
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY console/package.json console/
RUN --mount=type=cache,id=pnpm,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile --filter console
COPY console console
RUN pnpm --dir console build

# ── server ──────────────────────────────────────────────────────────────
FROM rust:1-bookworm AS server
# The Solami SDK builds protoc from source.
RUN apt-get update && apt-get install -y --no-install-recommends cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p gapless-server && \
    cp target/release/gapless-server /usr/local/bin/gapless-server

# ── runtime ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server /usr/local/bin/gapless-server /usr/local/bin/gapless-server
COPY --from=console /src/console/dist /app/console
COPY fixtures/pumpfun-150s.bin.zst /app/fixtures/pumpfun-150s.bin.zst
COPY docker/entrypoint.sh /usr/local/bin/gapless
RUN mkdir -p /data
VOLUME /data
EXPOSE 8790
ENTRYPOINT ["gapless"]
