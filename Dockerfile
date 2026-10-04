# The console. Only its build output goes on; Node is not in the final image.
# node:22-slim, pinned by digest.
FROM node:22-slim@sha256:43ac6c60b8f89723f746e8a92ce91abd5017e627ce1ddfe4238355d3a30b772c AS console
WORKDIR /app/ui
RUN corepack enable
COPY ui/package.json ui/pnpm-lock.yaml ui/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY ui ./
RUN pnpm build

# rust:1.94-slim-trixie, pinned by digest.
FROM rust:1.94-slim-trixie@sha256:cf09adf8c3ebaba10779e5c23ff7fe4df4cccdab8a91f199b0c142c53fef3e1a AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# Before the build: the gateway's build script compiles it into the binary.
COPY --from=console /app/ui/dist ./ui/dist
RUN cargo build --release --locked -p ultrafast-gateway

# Same Debian release as the builder, so the binary finds the C library it was linked against.
# debian:trixie-slim, pinned by digest.
FROM debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /var/lib/ultrafast ultrafast
COPY --from=builder /app/target/release/ultrafast /usr/local/bin/ultrafast
USER ultrafast
ENV UF_DATA_DIR=/var/lib/ultrafast UF_HOST=0.0.0.0 UF_PORT=3000
VOLUME /var/lib/ultrafast
EXPOSE 3000
ENTRYPOINT ["ultrafast"]
CMD ["serve"]
