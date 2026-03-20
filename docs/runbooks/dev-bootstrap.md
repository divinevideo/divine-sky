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

For the fuller ATProto provisioning lab, use `deploy/localnet/` instead. That profile is a second dev path with dedicated PLC, PDS, Jetstream, DNS, and handle-admin slices. Keep `config/docker-compose.yml` as the default when you only need the fast bridge-centric stack.

## Required Runtime Env

The bridge runtime now expects:

```bash
export PDS_AUTH_TOKEN=local-dev-token
export PLC_DIRECTORY_URL=http://127.0.0.1:2583
export HANDLE_DOMAIN=divine.video
export RELAY_SOURCE_NAME=local-stack-relay
```

`config/docker-compose.yml` sets these values for bridge startup in the local stack. It does not provide a dedicated PLC mock, so end-to-end provisioning still requires overriding `PLC_DIRECTORY_URL` to a real or test PLC endpoint when you exercise the opt-in flow.

The localnet lab in `deploy/localnet/` exists for that fuller provisioning path. It uses `divine.test` for local handles and expects bridge plus handle-gateway to consume localnet env overrides rather than branching runtime code.

## ATProto Opt-In Flow

The ATProto path is opt-in. A username claim alone only enables NIP-05.

To exercise the full provisioning flow locally, run the sibling repos that own:

- `../keycast` for consent and `/api/user/atproto/*`
- `../divine-name-server` for the public username read model
- `../divine-router` for read-only `/.well-known/atproto-did`

`divine-handle-gateway` does not expose a public `/.well-known/atproto-did` route.

When running `divine-handle-gateway` locally, set:

```bash
export DATABASE_URL=postgres://...
export KEYCAST_ATPROTO_TOKEN=local-keycast-token
export ATPROTO_PROVISIONING_URL=http://127.0.0.1:3200/provision
export ATPROTO_PROVISIONING_TOKEN=local-provisioning-token
export ATPROTO_KEYCAST_SYNC_URL=http://127.0.0.1:3000/api/internal/atproto/state
export ATPROTO_NAME_SERVER_SYNC_URL=http://127.0.0.1:8787/api/internal/username/set-atproto
export ATPROTO_NAME_SERVER_SYNC_TOKEN=local-sync-token
```

Use environment-specific local URLs for the provisioning worker, keycast internal sync route, and name-server.

When running `divine-atbridge` locally for provisioning, also set:

```bash
export HEALTH_BIND_ADDR=127.0.0.1:3200
export ATPROTO_PROVISIONING_TOKEN=local-provisioning-token
```

The `HEALTH_BIND_ADDR` listener now serves both `/health` and the internal `POST /provision` endpoint that `divine-handle-gateway` calls.

## Staging And Production Deploy Contract

Staging and production deploys for `divine-sky` are owned by `../divine-iac-coreconfig`, not this repository. The runtime contract in this repo is:

- `divine-atbridge`: internal worker in the shared `sky` namespace
- `divine-handle-gateway`: internal HTTP service in the shared `sky` namespace
- `divine-feedgen`: public HTTP/XRPC service
- `divine-labeler`: public ATProto label-query service

Only `divine-feedgen` and `divine-labeler` should have public Gateway API exposure. `divine-atbridge` and `divine-handle-gateway` remain cluster-internal.

Public hostnames are:

- staging feed: `feed.staging.dvines.org`
- production feed: `feed.divine.video`
- staging labeler: `labeler.staging.dvines.org`
- production labeler: `labeler.divine.video`

The deploy manifests, secrets, routes, and ArgoCD applications live in `divine-iac-coreconfig`. This repo only needs to keep the runtime bind addresses, health endpoints, and env contracts consistent with those manifests.

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

For the ATProto path specifically:

- username claim should succeed before ATProto is enabled
- `divine-handle-gateway` should persist pending/ready/failed/disabled lifecycle state in PostgreSQL
- `divine-handle-gateway` should sync final lifecycle state back into keycast via `/api/internal/atproto/state`
- `divine-atbridge` should only publish when `crosspost_enabled && provisioning_state == "ready"`

## Operator Bootstrap

For a clean local bring-up:

1. Install `libpq` and verify `cargo --version` and `docker compose version`.
2. Start the stack with `docker compose -f config/docker-compose.yml up -d`.
3. Wait for `postgres`, `minio`, `mock-blossom`, `mock-relay`, and `pds` healthchecks to pass.
4. Run `bash scripts/test-workspace.sh`.
5. Start the bridge locally with `cargo run -p divine-atbridge` if you are not using the compose-managed `bridge` service.
