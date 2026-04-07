# Debian Package Design for anyllm-proxy

**Date:** 2026-04-06
**Status:** Draft

## Overview

Add `.deb` package generation for `anyllm-proxy` using `cargo-deb`. Packages are built on native Linux runners (amd64, arm64), tested in CI, and attached to GitHub releases on version tags.

## Package Metadata

Added via `[package.metadata.deb]` in `crates/proxy/Cargo.toml`:

| Field | Value |
|-------|-------|
| Name | `anyllm-proxy` |
| Section | `net` |
| Priority | `optional` |
| Depends | `libc6` (libssl only if not statically linked) |
| Maintainer | From crate authors |
| License | MIT |

### Assets

| Source | Destination | Permissions |
|--------|-------------|-------------|
| `target/release/anyllm_proxy` | `/usr/bin/anyllm-proxy` | 755 |
| `packaging/anyllm-proxy.service` | `/lib/systemd/system/anyllm-proxy.service` | 644 |
| `packaging/anyllm-proxy.default` | `/etc/default/anyllm-proxy` | 640 (conffile) |

### Maintainer Scripts

- **postinst:** Creates `anyllm` system user/group, creates `/var/lib/anyllm` owned by `anyllm:anyllm`, runs `systemctl daemon-reload`
- **prerm:** Stops the service (`systemctl stop anyllm-proxy || true`)

Both scripts use `set -e` and check `$1` for standard Debian arguments (`configure`, `remove`).

## Packaging Files

New directory: `packaging/`

### `packaging/anyllm-proxy.service`

```ini
[Unit]
Description=AnYLLM Translation Proxy
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=anyllm
Group=anyllm
EnvironmentFile=-/etc/default/anyllm-proxy
ExecStart=/usr/bin/anyllm-proxy
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/anyllm
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

### `packaging/anyllm-proxy.default`

```bash
# /etc/default/anyllm-proxy
# Environment variables for anyllm-proxy

ANYLLM_HOME=/var/lib/anyllm
LISTEN_PORT=3000
# OPENAI_API_KEY=
# BACKEND=openai
# RUST_LOG=info
```

### `packaging/postinst`

Creates system user, data directory, reloads systemd. Only acts on `configure`.

### `packaging/prerm`

Stops the service. Only acts on `remove`.

## CI Integration

### Build (in `build-release` job, Linux targets only)

After the existing binary build step:

1. `cargo install cargo-deb` (cached via rust-cache)
2. `cargo deb -p anyllm_proxy --no-build` (reuses already-built binary)
3. Upload `.deb` artifact

Runs on both Linux matrix entries:
- `x86_64-unknown-linux-gnu` (ubuntu-latest)
- `aarch64-unknown-linux-gnu` (ubuntu-24.04-arm)

### Test (`test-deb` job, depends on `build-release`)

Matrix: amd64 (ubuntu-latest), arm64 (ubuntu-24.04-arm)

Checks:
1. `dpkg-deb --info` and `dpkg-deb --contents` (structure verification)
2. `lintian` (Debian policy lint, non-blocking warnings)
3. `sudo dpkg -i *.deb && sudo apt-get install -f -y` (install with dep resolution)
4. `anyllm-proxy --help` exits 0
5. `systemd-analyze verify anyllm-proxy.service` passes
6. User `anyllm` exists, `/var/lib/anyllm` exists with correct ownership

### Release (new `release-assets` job or added to `publish`)

On version tags (`v*`):
- Upload debs to GitHub release via `gh release upload`
- Naming: `anyllm-proxy_<version>_amd64.deb`, `anyllm-proxy_<version>_arm64.deb`

## Architectures

| Arch | Runner | Deb arch |
|------|--------|----------|
| x86_64 | ubuntu-latest | amd64 |
| aarch64 | ubuntu-24.04-arm | arm64 |

Both built natively (no cross-compilation).

## Out of Scope

- RPM packaging (can add later via nfpm or similar)
- APT repository hosting
- Uploading existing binary tarballs to GitHub releases (separate effort)
- logrotate, shell completions, man pages
