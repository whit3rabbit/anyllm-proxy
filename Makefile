VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
ARCH    := $(shell uname -m)

.PHONY: help build release deb package clean

help:
	@printf "%-12s %s\n" build   "debug build"
	@printf "%-12s %s\n" release "release build (current platform)"
	@printf "%-12s %s\n" deb     "build .deb package (Linux only, requires cargo-deb)"
	@printf "%-12s %s\n" package "create macOS tarball for Homebrew (macOS only)"
	@printf "%-12s %s\n" clean   "remove build and dist artifacts"
	@printf "\nversion: $(VERSION)\n"

build:
	cargo build -p anyllm_proxy

release:
	cargo build --release -p anyllm_proxy

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
