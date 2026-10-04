# syntax=docker/dockerfile:1.7
# MinRegistry: one image, one binary (the web UI is embedded at build time).

ARG RUST_VERSION=1
ARG NODE_VERSION=24
ARG PNPM_VERSION=12.9.1

FROM node:${NODE_VERSION}-bookworm-slim AS web
ARG PNPM_VERSION
RUN npm install -g pnpm@${PNPM_VERSION}
WORKDIR /src/web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml* web/.npmrc* ./
RUN --mount=type=cache,id=minregistry-pnpm,target=/root/.cache/pnpm-store \
    pnpm config set store-dir /root/.cache/pnpm-store && pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

FROM rust:${RUST_VERSION}-bookworm AS server
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .sqlx .sqlx
COPY server server
COPY --from=web /src/web/dist web/dist
ENV SQLX_OFFLINE=true
RUN --mount=type=cache,id=minregistry-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=minregistry-target,target=/src/target \
    cargo build --release --locked -p minregistry \
    && cp target/release/minregistry /usr/local/bin/minregistry

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin minregistry \
    && mkdir -p /data \
    && chown minregistry /data
COPY --from=server /usr/local/bin/minregistry /usr/local/bin/minregistry
ENV MINREGISTRY_LISTEN=0.0.0.0:5000 \
    MINREGISTRY_DB_PATH=/data/minregistry.db \
    MINREGISTRY_FS_ROOT=/data/blobs \
    MINREGISTRY_UPLOAD_DIR=/data/uploads \
    MINREGISTRY_LOG_FORMAT=json
USER 10001
VOLUME ["/data"]
EXPOSE 5000
ENTRYPOINT ["minregistry"]
CMD ["serve"]
