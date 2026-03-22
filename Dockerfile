FROM rust:1.85-slim AS builder
RUN apt-get update && apt-get install -y --no-install-recommends libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release -p anthropic_openai_proxy

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/anthropic_openai_proxy /usr/local/bin/
EXPOSE 3000
ENTRYPOINT ["anthropic_openai_proxy"]
