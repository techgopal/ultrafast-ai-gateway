FROM rust:1.94-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p ultrafast-gateway

FROM debian:bookworm-slim
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
