# Debian Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `.deb` packages for `anyllm-proxy` on amd64 and arm64, test them in CI, and attach to GitHub releases on version tags.

**Architecture:** `cargo-deb` reads `[package.metadata.deb]` from `crates/proxy/Cargo.toml` and produces a `.deb` from the already-built release binary. Packaging assets (systemd unit, env defaults, maintainer scripts) live in `packaging/`. CI builds debs on native Linux runners, runs install verification in a separate job, then uploads to GitHub releases.

**Tech Stack:** cargo-deb, dpkg, lintian, systemd, GitHub Actions

---

### Task 1: Create packaging assets

**Files:**
- Create: `packaging/anyllm-proxy.service`
- Create: `packaging/anyllm-proxy.default`
- Create: `packaging/postinst`
- Create: `packaging/prerm`

- [ ] **Step 1: Create the systemd unit file**

Create `packaging/anyllm-proxy.service`:

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

- [ ] **Step 2: Create the environment defaults file**

Create `packaging/anyllm-proxy.default`:

```bash
# /etc/default/anyllm-proxy
# Environment variables for anyllm-proxy
# See: https://github.com/whit3rabbit/anyllm-proxy

ANYLLM_HOME=/var/lib/anyllm
LISTEN_PORT=3000
# OPENAI_API_KEY=
# BACKEND=openai
# RUST_LOG=info
```

- [ ] **Step 3: Create the postinst script**

Create `packaging/postinst`:

```bash
#!/bin/sh
set -e

case "$1" in
    configure)
        # Create system user if it does not exist
        if ! getent passwd anyllm >/dev/null 2>&1; then
            adduser --system --group --home /var/lib/anyllm --no-create-home --quiet anyllm
        fi

        # Create data directory
        install -d -o anyllm -g anyllm -m 0750 /var/lib/anyllm

        # Reload systemd to pick up the new unit file
        if [ -d /run/systemd/system ]; then
            systemctl daemon-reload || true
        fi
        ;;
esac

exit 0
```

- [ ] **Step 4: Create the prerm script**

Create `packaging/prerm`:

```bash
#!/bin/sh
set -e

case "$1" in
    remove|deconfigure)
        if [ -d /run/systemd/system ]; then
            systemctl stop anyllm-proxy || true
        fi
        ;;
esac

exit 0
```

- [ ] **Step 5: Make maintainer scripts executable**

```bash
chmod +x packaging/postinst packaging/prerm
```

- [ ] **Step 6: Commit**

```bash
git add packaging/
git commit -m "feat: add deb packaging assets (systemd, env defaults, maintainer scripts)"
```

---

### Task 2: Add cargo-deb metadata to Cargo.toml

**Files:**
- Modify: `crates/proxy/Cargo.toml` (append after line 95, after `[dev-dependencies]` section)

- [ ] **Step 1: Add `[package.metadata.deb]` section**

Append to `crates/proxy/Cargo.toml` (after the `[dev-dependencies]` block):

```toml
[package.metadata.deb]
name = "anyllm-proxy"
section = "net"
priority = "optional"
depends = "libc6 (>= 2.31)"
maintainer-scripts = "../../packaging"
assets = [
    # Binary
    ["target/release/anyllm_proxy", "/usr/bin/anyllm-proxy", "755"],
    # Systemd unit
    ["../../packaging/anyllm-proxy.service", "/lib/systemd/system/anyllm-proxy.service", "644"],
]
conf-files = ["/etc/default/anyllm-proxy"]
# The default file is installed via assets but marked as conffile so dpkg preserves edits
extended-description = """HTTP proxy that accepts Anthropic Messages API and OpenAI Chat Completions requests, translates between formats, and forwards to any supported backend. Supports streaming SSE, tool calling, virtual key management, and cost tracking."""

[package.metadata.deb.variants.default]
assets = [
    ["target/release/anyllm_proxy", "/usr/bin/anyllm-proxy", "755"],
    ["../../packaging/anyllm-proxy.service", "/lib/systemd/system/anyllm-proxy.service", "644"],
    ["../../packaging/anyllm-proxy.default", "/etc/default/anyllm-proxy", "640"],
]
```

Wait -- `cargo-deb` does not use a `variants.default` section for conffile assets. The simpler approach: include the default file in the main `assets` array. `conf-files` tells dpkg to treat it as a conffile.

Corrected: just use the top-level `assets` array with all three entries.

```toml
[package.metadata.deb]
name = "anyllm-proxy"
section = "net"
priority = "optional"
depends = "libc6 (>= 2.31)"
maintainer-scripts = "../../packaging"
conf-files = ["/etc/default/anyllm-proxy"]
extended-description = """\
HTTP proxy that accepts Anthropic Messages API and OpenAI Chat Completions \
requests, translates between formats, and forwards to any supported backend. \
Supports streaming SSE, tool calling, virtual key management, and cost tracking."""
assets = [
    ["target/release/anyllm_proxy", "/usr/bin/anyllm-proxy", "755"],
    ["../../packaging/anyllm-proxy.service", "/lib/systemd/system/anyllm-proxy.service", "644"],
    ["../../packaging/anyllm-proxy.default", "/etc/default/anyllm-proxy", "640"],
]
```

- [ ] **Step 2: Verify cargo-deb metadata parses**

```bash
cargo install cargo-deb --locked --quiet
cargo deb -p anyllm_proxy --no-build --no-strip 2>&1 || true
```

Expected: either produces a `.deb` (if a release binary exists) or fails with "file not found" for the binary (not a metadata parse error). If you see a metadata error, fix the `Cargo.toml` section.

- [ ] **Step 3: Build a test deb locally**

```bash
cargo build --release -p anyllm_proxy
cargo deb -p anyllm_proxy --no-build --no-strip
```

Expected: produces `target/debian/anyllm-proxy_0.2.0_<arch>.deb`

- [ ] **Step 4: Inspect the deb (Linux only, skip on macOS)**

```bash
dpkg-deb --info target/debian/anyllm-proxy_*.deb
dpkg-deb --contents target/debian/anyllm-proxy_*.deb
```

Expected output should show:
- Package: `anyllm-proxy`
- `/usr/bin/anyllm-proxy` (755)
- `/lib/systemd/system/anyllm-proxy.service` (644)
- `/etc/default/anyllm-proxy` (640)
- Maintainer scripts: `postinst`, `prerm`

On macOS, `dpkg-deb` is not available. The `.deb` file itself is still produced correctly by `cargo deb`; inspection will be done in CI.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/Cargo.toml
git commit -m "feat: add cargo-deb metadata for .deb package generation"
```

---

### Task 3: Add deb build step to CI workflow

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add `deb_arch` to Linux matrix entries**

In the `build-release` job matrix, add a `deb_arch` field to the two Linux entries so we can name the artifact correctly. Non-Linux entries get no `deb_arch`.

Replace the matrix `include` block (lines 75-95) with:

```yaml
        include:
          # Linux x86_64
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            binary: anyllm_proxy
            deb_arch: amd64
          # Linux ARM64 (native GitHub runner)
          - os: ubuntu-24.04-arm
            target: aarch64-unknown-linux-gnu
            binary: anyllm_proxy
            deb_arch: arm64
          # macOS Apple Silicon
          - os: macos-latest
            target: aarch64-apple-darwin
            binary: anyllm_proxy
          # macOS Intel
          - os: macos-latest
            target: x86_64-apple-darwin
            binary: anyllm_proxy
          # Windows x86_64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            binary: anyllm_proxy.exe
```

- [ ] **Step 2: Add cargo-deb build step after binary build**

After the existing "Build" step (line 111) and before "Upload artifact" (line 112), add:

```yaml
      - name: Build deb package
        if: matrix.deb_arch != ''
        run: |
          cargo install cargo-deb --locked --quiet
          cargo deb -p anyllm_proxy --no-build --no-strip --target ${{ matrix.target }}
      - name: Upload deb artifact
        if: matrix.deb_arch != ''
        uses: actions/upload-artifact@v4
        with:
          name: anyllm-proxy-deb-${{ matrix.deb_arch }}
          path: target/${{ matrix.target }}/debian/*.deb
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: build deb packages on Linux targets"
```

---

### Task 4: Add deb test job to CI workflow

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add `test-deb` job**

Add after the `build-release` job and before the `publish` job:

```yaml
  test-deb:
    name: Test deb (${{ matrix.arch }})
    needs: build-release
    if: startsWith(github.ref, 'refs/tags/v')
    strategy:
      fail-fast: false
      matrix:
        include:
          - arch: amd64
            os: ubuntu-latest
          - arch: arm64
            os: ubuntu-24.04-arm
    runs-on: ${{ matrix.os }}
    steps:
      - name: Download deb
        uses: actions/download-artifact@v4
        with:
          name: anyllm-proxy-deb-${{ matrix.arch }}
          path: ./deb

      - name: Inspect package
        run: |
          dpkg-deb --info ./deb/*.deb
          dpkg-deb --contents ./deb/*.deb

      - name: Lint package
        run: |
          sudo apt-get update -qq
          sudo apt-get install -y -qq lintian
          lintian --no-tag-display-limit ./deb/*.deb || true

      - name: Install package
        run: |
          sudo dpkg -i ./deb/*.deb || true
          sudo apt-get install -f -y

      - name: Verify binary
        run: |
          test -x /usr/bin/anyllm-proxy
          # Binary exists and is executable; no --help flag available,
          # so just check it is a valid ELF binary
          file /usr/bin/anyllm-proxy | grep -q "ELF"

      - name: Verify systemd unit
        run: |
          systemd-analyze verify /lib/systemd/system/anyllm-proxy.service

      - name: Verify postinst artifacts
        run: |
          # Run postinst manually since dpkg may not trigger it fully in CI
          sudo /var/lib/dpkg/info/anyllm-proxy.postinst configure || true
          getent passwd anyllm
          test -d /var/lib/anyllm
          stat -c '%U:%G' /var/lib/anyllm | grep -q 'anyllm:anyllm'

      - name: Verify config file
        run: |
          test -f /etc/default/anyllm-proxy
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add deb install verification job"
```

---

### Task 5: Add release asset upload to CI

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add `release-assets` job**

Add after the `test-deb` job:

```yaml
  release-assets:
    name: Upload release assets
    needs: [test-deb]
    if: startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - name: Download amd64 deb
        uses: actions/download-artifact@v4
        with:
          name: anyllm-proxy-deb-amd64
          path: ./debs
      - name: Download arm64 deb
        uses: actions/download-artifact@v4
        with:
          name: anyllm-proxy-deb-arm64
          path: ./debs
      - name: Upload to GitHub Release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          ls -la ./debs/
          gh release upload "${{ github.ref_name }}" ./debs/*.deb --repo "${{ github.repository }}"
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: upload deb packages to GitHub releases"
```

---

### Task 6: Update CLAUDE.md and verify

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add deb packaging section to CLAUDE.md**

Add after the "Docker Smoke Tests" section:

```markdown
## Debian Package

Built with `cargo-deb`. On version tags, CI builds `.deb` packages for amd64 and arm64, tests them (install + verify), and uploads to GitHub releases.

Local build:
\```bash
cargo build --release -p anyllm_proxy
cargo deb -p anyllm_proxy --no-build --no-strip
# Output: target/debian/anyllm-proxy_<version>_<arch>.deb
\```

Package contents:
- `/usr/bin/anyllm-proxy` (binary)
- `/lib/systemd/system/anyllm-proxy.service` (systemd unit)
- `/etc/default/anyllm-proxy` (environment config, conffile)
- Creates `anyllm` system user and `/var/lib/anyllm` data directory on install

After installing: `sudo systemctl enable --now anyllm-proxy`, then edit `/etc/default/anyllm-proxy` with your API keys.
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add deb packaging section to CLAUDE.md"
```

- [ ] **Step 3: Final check -- review the full CI workflow for consistency**

```bash
cat .github/workflows/ci.yml
```

Verify:
- `build-release` has `deb_arch` on Linux entries only
- `test-deb` depends on `build-release`
- `release-assets` depends on `test-deb`
- `publish` does not depend on `test-deb` or `release-assets` (crates.io publish is independent)
- Artifact names match between upload and download steps
