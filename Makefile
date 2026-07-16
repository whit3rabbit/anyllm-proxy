VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
ARCH    := $(shell uname -m)

.PHONY: help build release deb package clean test test-all-features lint fmt audit run run-webui \
        build-all-features build-otel build-qdrant build-redis build-optimizer \
        ui-install ui-build ui-dev ui-test ui-lint

help:
	@printf "%-22s %s\n" "build" "debug build of proxy with default features"
	@printf "%-22s %s\n" "build-all-features" "debug build of proxy with all features enabled"
	@printf "%-22s %s\n" "build-otel" "debug build of proxy with OpenTelemetry feature"
	@printf "%-22s %s\n" "build-qdrant" "debug build of proxy with Qdrant feature"
	@printf "%-22s %s\n" "build-redis" "debug build of proxy with Redis feature"
	@printf "%-22s %s\n" "build-optimizer" "debug build of proxy with optimizer-onnx feature"
	@printf "%-22s %s\n" "release" "release build (current platform, default features)"
	@printf "%-22s %s\n" "release-all-features" "release build (current platform, all features)"
	@printf "%-22s %s\n" "test" "run all workspace tests with default features"
	@printf "%-22s %s\n" "test-all-features" "run all workspace tests with all features"
	@printf "%-22s %s\n" "lint" "run clippy and check formatting on workspace"
	@printf "%-22s %s\n" "fmt" "format all Rust source code"
	@printf "%-22s %s\n" "audit" "run security audit on dependencies"
	@printf "%-22s %s\n" "run" "run proxy locally (without UI)"
	@printf "%-22s %s\n" "run-webui" "run proxy locally with admin web UI enabled"
	@printf "%-22s %s\n" "ui-install" "install admin UI dependencies (npm)"
	@printf "%-22s %s\n" "ui-build" "build admin UI (generates dist/)"
	@printf "%-22s %s\n" "ui-dev" "run admin UI dev server"
	@printf "%-22s %s\n" "ui-test" "run admin UI behavior tests"
	@printf "%-22s %s\n" "ui-lint" "lint and typecheck admin UI"
	@printf "%-22s %s\n" "deb" "build .deb package (Linux only, requires cargo-deb)"
	@printf "%-22s %s\n" "package" "create macOS tarball for Homebrew (macOS only)"
	@printf "%-22s %s\n" "clean" "remove build and dist artifacts"
	@printf "\nversion: $(VERSION)\n"

build:
	cargo build -p anyllm_proxy

build-all-features:
	cargo build -p anyllm_proxy --all-features

build-otel:
	cargo build -p anyllm_proxy --features otel

build-qdrant:
	cargo build -p anyllm_proxy --features qdrant

build-redis:
	cargo build -p anyllm_proxy --features redis

build-optimizer:
	cargo build -p anyllm_proxy --features optimizer-onnx

release:
	cargo build --release -p anyllm_proxy

release-all-features:
	cargo build --release -p anyllm_proxy --all-features

test:
	cargo test

test-all-features:
	cargo test --all-features

lint:
	cargo clippy --workspace --all-targets --all-features -- -D warnings
	cargo fmt --check

fmt:
	cargo fmt

audit:
	cargo audit

run:
	PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy

run-webui:
	PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy -- --webui

ui-install:
	cd crates/proxy/admin-ui && npm ci --legacy-peer-deps

ui-build:
	cd crates/proxy/admin-ui && npm run build

ui-dev:
	cd crates/proxy/admin-ui && npm run dev

ui-test:
	cd crates/proxy/admin-ui && npm run test

ui-lint:
	cd crates/proxy/admin-ui && npm run lint && npm run typecheck

deb: release
	cargo deb -p anyllm_proxy --no-build --no-strip

package: release
	@mkdir -p dist
	@cp target/release/anyllm_proxy dist/anyllm-proxy
	@tar -czf dist/anyllm-proxy-$(VERSION)-macos-$(ARCH).tar.gz -C dist anyllm-proxy
	@rm dist/anyllm-proxy
	@echo "dist/anyllm-proxy-$(VERSION)-macos-$(ARCH).tar.gz"

clean:
	cargo clean
	rm -rf dist/
	rm -rf crates/proxy/admin-ui/dist/

