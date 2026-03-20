# Dev Bootstrap

## Prerequisites

- Rust toolchain with `cargo`
- Docker and Docker Compose
- PostgreSQL client libraries

On macOS with Homebrew:

```bash
brew install libpq
```

On Debian or Ubuntu:

```bash
sudo apt-get update
sudo apt-get install -y libpq-dev pkg-config
```

## Local Infra

Start the shared local services:

```bash
docker compose -f config/docker-compose.yml up -d
```

This stack brings up PostgreSQL, MinIO, a mock Blossom server, a mock Nostr relay, a local PDS, and the bridge container.

## Required Runtime Env

The bridge runtime now expects:

```bash
export PDS_AUTH_TOKEN=local-dev-token
export RELAY_SOURCE_NAME=local-stack-relay
```

`config/docker-compose.yml` sets these values for the local stack. In non-compose environments, set them explicitly before running `cargo run -p divine-atbridge`.

## Workspace Verification

Use the checked-in bootstrap script instead of hand-exporting linker flags:

```bash
bash scripts/test-workspace.sh
```

The script auto-detects `libpq` from `pg_config` when available. On macOS, it first tries the default Homebrew `libpq` prefix at `/opt/homebrew/opt/libpq`, then falls back to the newest installed Cellar path if the `opt` symlink is missing. On Linux, it exports `LD_LIBRARY_PATH` from `pg_config` so the linker can find `libpq` during test runs.

If you still need to export the paths manually, use:

```bash
export PATH="/opt/homebrew/opt/libpq/bin:$PATH"
export LIBRARY_PATH="/opt/homebrew/opt/libpq/lib:${LIBRARY_PATH:-}"
export DYLD_FALLBACK_LIBRARY_PATH="/opt/homebrew/opt/libpq/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}"
export CPATH="/opt/homebrew/opt/libpq/include:${CPATH:-}"
export PKG_CONFIG_PATH="/opt/homebrew/opt/libpq/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
```

## Expected Result

`cargo check --workspace`, the focused crate tests, and `cargo test --workspace` should all pass from the repository root.

## Operator Bootstrap

For a clean local bring-up:

1. Install `libpq` and verify `cargo --version` and `docker compose version`.
2. Start the stack with `docker compose -f config/docker-compose.yml up -d`.
3. Wait for `postgres`, `minio`, `mock-blossom`, `mock-relay`, and `pds` healthchecks to pass.
4. Run `bash scripts/test-workspace.sh`.
5. Start the bridge locally with `cargo run -p divine-atbridge` if you are not using the compose-managed `bridge` service.
