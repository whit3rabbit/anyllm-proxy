# syntax=docker/dockerfile:1

# ── Stage 1: build frontend ───────────────────────────────────────────────────
FROM node:20-alpine AS frontend
WORKDIR /app/crates/proxy/admin-ui
COPY crates/proxy/admin-ui/package.json crates/proxy/admin-ui/package-lock.json ./
RUN npm ci --legacy-peer-deps
COPY crates/proxy/admin-ui/ ./
RUN npm run build

# ── Stage 2: install cargo-chef ───────────────────────────────────────────────
FROM rust:1-alpine AS chef
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static
RUN cargo install cargo-chef --locked
WORKDIR /app

# ── Stage 3: compute dependency recipe ───────────────────────────────────────
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 4: build dependencies (cached layer) + compile binary ───────────────
FROM chef AS builder
# OPENSSL_STATIC must be set before cook so openssl-sys links statically
# when building reqwest/native-tls dependencies.
ENV OPENSSL_STATIC=1
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p anyllm_proxy
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY assets assets
# Inject frontend build output so include_str!() resolves at compile time.
COPY --from=frontend /app/crates/proxy/admin-ui/dist/ crates/proxy/admin-ui/dist/
RUN cargo build --release -p anyllm_proxy

# ── Stage 5: minimal Alpine runtime ──────────────────────────────────────────
FROM alpine:3.21 AS runtime
RUN apk add --no-cache ca-certificates tzdata
RUN addgroup -S -g 1001 anyllm && adduser -S -u 1001 -G anyllm anyllm
WORKDIR /app
RUN chown anyllm:anyllm /app
COPY --from=builder /app/target/release/anyllm_proxy /usr/local/bin/anyllm_proxy
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
RUN mkdir /data && chown anyllm:anyllm /data
VOLUME ["/data"]
USER anyllm
EXPOSE 3000 3001
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
