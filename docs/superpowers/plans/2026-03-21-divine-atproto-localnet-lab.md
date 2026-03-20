# DiVine ATProto Localnet Lab Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an isolated ATProto local-network lab for end-to-end provisioning and protocol testing, with its own PLC, PDS, Jetstream, DNS, and handle admin tooling, without replacing the current bridge-centric local dev stack.

**Architecture:** Keep [config/docker-compose.yml](/Users/rabble/code/divine/divine-sky/config/docker-compose.yml) as the fast default for day-to-day bridge work. Add a new additive profile under `deploy/localnet/` that runs a dedicated PLC, a Tailscale-exposed PDS, Jetstream, split-DNS, and a small Rust handle-admin service for `*.divine.test` handles. Bridge and handle-gateway continue using their existing env contracts, but gain documented localnet-specific override files and smoke scripts.

**Tech Stack:** Docker Compose, Tailscale, Nginx, did-method-plc, `rsky-pds` with optional image override, Jetstream, CoreDNS, Rust workspace crates, Axum, PostgreSQL, MinIO, Bash.

---

## Scope And Guardrails

- The localnet lab is a second dev path, not a replacement for [config/docker-compose.yml](/Users/rabble/code/divine/divine-sky/config/docker-compose.yml).
- Keep production and staging contracts unchanged.
- Do not move `/.well-known/atproto-did` back into `divine-handle-gateway`; that route remains outside this repo's runtime contract.
- Use `divine.test` as the local handle suffix for the lab so local DNS does not pretend to be the production public domain.
- Default the lab PDS to `rsky-pds` for parity with the current repo, but make the image configurable so the lab can swap to the official Bluesky PDS if runtime compatibility work demands it.

## Planned Repository Layout

```text
deploy/
  localnet/
    README.md
    bridge.env.example
    handle-gateway.env.example
    plc/
      docker-compose.yml
      init.sql
      nginx.conf
      README.md
    pds/
      docker-compose.yml
      env.example
      nginx.conf
      README.md
    jetstream/
      docker-compose.yml
      env.example
      nginx.conf
      README.md
    dns/
      docker-compose.yml
      Corefile.example
      nginx.conf
      README.md
crates/
  divine-localnet-admin/
    Cargo.toml
    src/
      lib.rs
      main.rs
    tests/
      handle_records.rs
scripts/
  localnet-up.sh
  localnet-down.sh
  localnet-smoke.sh
```

## Chunk 1: Contract, Docs, And Core Services

### Task 1: Define The Localnet Contract And Add Static Verification

**Files:**
- Create: `crates/divine-atbridge/tests/localnet_contract.rs`
- Create: `deploy/localnet/README.md`
- Modify: `README.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/pds-operations.md`

- [ ] **Step 1: Write the failing contract test**

```rust
#[test]
fn localnet_docs_and_layout_are_present() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();
    assert!(repo_root.join("deploy/localnet/README.md").exists());
    let bootstrap = std::fs::read_to_string(repo_root.join("docs/runbooks/dev-bootstrap.md")).unwrap();
    assert!(bootstrap.contains("deploy/localnet"));
}
```

- [ ] **Step 2: Run the new test and verify it fails**

Run: `cargo test -p divine-atbridge localnet_docs_and_layout_are_present -- --nocapture`
Expected: FAIL because `deploy/localnet/README.md` and the runbook references do not exist yet.

- [ ] **Step 3: Document the second dev path**

Add [deploy/localnet/README.md](/Users/rabble/code/divine/divine-sky/deploy/localnet/README.md) with:
- the service list: PLC, PDS, Jetstream, DNS, handle admin
- the rationale for keeping the current stack as the default
- the local handle suffix `divine.test`
- the rule that bridge and handle-gateway consume env overrides rather than code-path forks

Update [README.md](/Users/rabble/code/divine/divine-sky/README.md), [docs/runbooks/dev-bootstrap.md](/Users/rabble/code/divine/divine-sky/docs/runbooks/dev-bootstrap.md), and [docs/runbooks/pds-operations.md](/Users/rabble/code/divine/divine-sky/docs/runbooks/pds-operations.md) to distinguish:
- the fast stack in `config/docker-compose.yml`
- the full localnet lab in `deploy/localnet/`

- [ ] **Step 4: Re-run the new contract test**

Run: `cargo test -p divine-atbridge localnet_docs_and_layout_are_present -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/divine-atbridge/tests/localnet_contract.rs deploy/localnet/README.md README.md docs/runbooks/dev-bootstrap.md docs/runbooks/pds-operations.md
git commit -m "docs: define localnet lab contract"
```

### Task 2: Add The PLC And PDS Localnet Slices

**Files:**
- Create: `deploy/localnet/plc/docker-compose.yml`
- Create: `deploy/localnet/plc/init.sql`
- Create: `deploy/localnet/plc/nginx.conf`
- Create: `deploy/localnet/plc/README.md`
- Create: `deploy/localnet/pds/docker-compose.yml`
- Create: `deploy/localnet/pds/env.example`
- Create: `deploy/localnet/pds/nginx.conf`
- Create: `deploy/localnet/pds/README.md`
- Modify: `crates/divine-atbridge/tests/localnet_contract.rs`

- [ ] **Step 1: Extend the contract test with PLC and PDS assertions**

```rust
#[test]
fn localnet_plc_and_pds_compose_files_define_required_services() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();
    let plc = std::fs::read_to_string(repo_root.join("deploy/localnet/plc/docker-compose.yml")).unwrap();
    let pds = std::fs::read_to_string(repo_root.join("deploy/localnet/pds/docker-compose.yml")).unwrap();
    assert!(plc.contains("tailscale:"));
    assert!(plc.contains("app:"));
    assert!(pds.contains("PDS_DID_PLC_URL"));
    assert!(pds.contains("PDS_IMAGE"));
}
```

- [ ] **Step 2: Run the PLC/PDS contract test and verify it fails**

Run: `cargo test -p divine-atbridge localnet_plc_and_pds_compose_files_define_required_services -- --nocapture`
Expected: FAIL because the new compose files are not present yet.

- [ ] **Step 3: Create the PLC slice**

Implement [deploy/localnet/plc/docker-compose.yml](/Users/rabble/code/divine/divine-sky/deploy/localnet/plc/docker-compose.yml) using:
- `postgres`
- a built `did-method-plc` server image
- a `tailscale` sidecar
- an `nginx` TLS proxy on `network_mode: service:tailscale`

Use [deploy/localnet/plc/init.sql](/Users/rabble/code/divine/divine-sky/deploy/localnet/plc/init.sql) and [deploy/localnet/plc/nginx.conf](/Users/rabble/code/divine/divine-sky/deploy/localnet/plc/nginx.conf) to match the internal hostname pattern documented in the README.

- [ ] **Step 4: Create the PDS slice**

Implement [deploy/localnet/pds/docker-compose.yml](/Users/rabble/code/divine/divine-sky/deploy/localnet/pds/docker-compose.yml) with:
- `PDS_IMAGE=${PDS_IMAGE:-ghcr.io/blacksky-algorithms/rsky-pds:latest}`
- `minio` plus bucket bootstrap reuse via [config/minio-init.sh](/Users/rabble/code/divine/divine-sky/config/minio-init.sh)
- `PDS_DID_PLC_URL=https://plc.<tailnet>.ts.net`
- a `tailscale` sidecar and `nginx` proxy

Document all required placeholders in [deploy/localnet/pds/env.example](/Users/rabble/code/divine/divine-sky/deploy/localnet/pds/env.example), including:
- `PDS_HOSTNAME`
- `PDS_SERVICE_DID`
- `PDS_SERVICE_HANDLE_DOMAINS=.divine.test`
- `PDS_ADMIN_PASSWORD`
- `PDS_DID_PLC_URL`
- `PDS_IMAGE`

- [ ] **Step 5: Validate both compose files**

Run: `docker compose -f deploy/localnet/plc/docker-compose.yml config`
Expected: exit code `0`

Run: `docker compose -f deploy/localnet/pds/docker-compose.yml --env-file deploy/localnet/pds/env.example config`
Expected: exit code `0`

- [ ] **Step 6: Re-run the PLC/PDS contract test**

Run: `cargo test -p divine-atbridge localnet_plc_and_pds_compose_files_define_required_services -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add deploy/localnet/plc deploy/localnet/pds crates/divine-atbridge/tests/localnet_contract.rs
git commit -m "feat: add localnet plc and pds slices"
```

### Task 3: Add Jetstream And DNS Scaffolding

**Files:**
- Create: `deploy/localnet/jetstream/docker-compose.yml`
- Create: `deploy/localnet/jetstream/env.example`
- Create: `deploy/localnet/jetstream/nginx.conf`
- Create: `deploy/localnet/jetstream/README.md`
- Create: `deploy/localnet/dns/docker-compose.yml`
- Create: `deploy/localnet/dns/Corefile.example`
- Create: `deploy/localnet/dns/nginx.conf`
- Create: `deploy/localnet/dns/README.md`
- Modify: `crates/divine-atbridge/tests/localnet_contract.rs`

- [ ] **Step 1: Add failing Jetstream and DNS assertions**

```rust
#[test]
fn localnet_jetstream_and_dns_slices_are_defined() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();
    let jetstream = std::fs::read_to_string(repo_root.join("deploy/localnet/jetstream/docker-compose.yml")).unwrap();
    let dns = std::fs::read_to_string(repo_root.join("deploy/localnet/dns/docker-compose.yml")).unwrap();
    assert!(jetstream.contains("JETSTREAM_WS_URL"));
    assert!(dns.contains("coredns:"));
    assert!(dns.contains("app:"));
}
```

- [ ] **Step 2: Run the Jetstream/DNS test and verify it fails**

Run: `cargo test -p divine-atbridge localnet_jetstream_and_dns_slices_are_defined -- --nocapture`
Expected: FAIL because the files do not exist yet.

- [ ] **Step 3: Create the Jetstream slice**

Implement [deploy/localnet/jetstream/docker-compose.yml](/Users/rabble/code/divine/divine-sky/deploy/localnet/jetstream/docker-compose.yml) with:
- `JETSTREAM_WS_URL=wss://pds.<tailnet>.ts.net/xrpc/com.atproto.sync.subscribeRepos`
- a `tailscale` sidecar
- an `nginx` proxy

Document usage in [deploy/localnet/jetstream/README.md](/Users/rabble/code/divine/divine-sky/deploy/localnet/jetstream/README.md) and place defaults in [deploy/localnet/jetstream/env.example](/Users/rabble/code/divine/divine-sky/deploy/localnet/jetstream/env.example).

- [ ] **Step 4: Create the DNS slice without production coupling**

Implement [deploy/localnet/dns/docker-compose.yml](/Users/rabble/code/divine/divine-sky/deploy/localnet/dns/docker-compose.yml) with:
- `coredns`
- `app` reserved for the local handle-admin service added in Chunk 2
- a `tailscale` sidecar
- an `nginx` proxy

Use [deploy/localnet/dns/Corefile.example](/Users/rabble/code/divine/divine-sky/deploy/localnet/dns/Corefile.example) to serve:
- wildcard `A` responses for `*.divine.test`
- `_atproto.<name>.divine.test` TXT records
- a local admin hostname on the tailnet

- [ ] **Step 5: Validate the compose files**

Run: `docker compose -f deploy/localnet/jetstream/docker-compose.yml --env-file deploy/localnet/jetstream/env.example config`
Expected: exit code `0`

Run: `docker compose -f deploy/localnet/dns/docker-compose.yml config`
Expected: exit code `0`

- [ ] **Step 6: Re-run the Jetstream/DNS contract test**

Run: `cargo test -p divine-atbridge localnet_jetstream_and_dns_slices_are_defined -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add deploy/localnet/jetstream deploy/localnet/dns crates/divine-atbridge/tests/localnet_contract.rs
git commit -m "feat: add localnet jetstream and dns scaffolding"
```

## Chunk 2: Handle Admin, Integration Overrides, And Operator Flow

### Task 4: Add A Rust Handle-Admin Service For The Localnet DNS Slice

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/divine-localnet-admin/Cargo.toml`
- Create: `crates/divine-localnet-admin/src/lib.rs`
- Create: `crates/divine-localnet-admin/src/main.rs`
- Create: `crates/divine-localnet-admin/tests/handle_records.rs`
- Modify: `deploy/localnet/dns/docker-compose.yml`
- Modify: `deploy/localnet/dns/README.md`

- [ ] **Step 1: Write the failing handle-admin test**

```rust
#[tokio::test]
async fn creates_handle_record_for_divine_test() {
    let app = divine_localnet_admin::app_with_state_for_tests();
    let response = app
        .oneshot(/* POST /api/handles with alice + did */)
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
}
```

- [ ] **Step 2: Run the crate test and verify it fails**

Run: `cargo test -p divine-localnet-admin creates_handle_record_for_divine_test -- --nocapture`
Expected: FAIL because the new crate does not exist yet.

- [ ] **Step 3: Add the new workspace crate**

Create [crates/divine-localnet-admin/Cargo.toml](/Users/rabble/code/divine/divine-sky/crates/divine-localnet-admin/Cargo.toml) and add it to [Cargo.toml](/Users/rabble/code/divine/divine-sky/Cargo.toml).

Implement [crates/divine-localnet-admin/src/lib.rs](/Users/rabble/code/divine/divine-sky/crates/divine-localnet-admin/src/lib.rs) and [crates/divine-localnet-admin/src/main.rs](/Users/rabble/code/divine/divine-sky/crates/divine-localnet-admin/src/main.rs) as a minimal Axum app that:
- exposes `/health`
- exposes `POST /api/handles`
- exposes `GET /api/handles/:name`
- persists handle-to-DID mappings in a small SQLite or JSON-backed store under a mounted volume
- rewrites a CoreDNS data file or zone fragment that the DNS container can read

- [ ] **Step 4: Wire the DNS compose file to the new service**

Update [deploy/localnet/dns/docker-compose.yml](/Users/rabble/code/divine/divine-sky/deploy/localnet/dns/docker-compose.yml) so `app` runs the new Rust crate instead of a placeholder image. Update [deploy/localnet/dns/README.md](/Users/rabble/code/divine/divine-sky/deploy/localnet/dns/README.md) with the local admin URL and record format.

- [ ] **Step 5: Run the new crate tests**

Run: `cargo test -p divine-localnet-admin -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/divine-localnet-admin deploy/localnet/dns
git commit -m "feat: add localnet handle admin service"
```

### Task 5: Add Bridge And Handle-Gateway Localnet Overrides

**Files:**
- Create: `deploy/localnet/bridge.env.example`
- Create: `deploy/localnet/handle-gateway.env.example`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/atproto-opt-in-smoke-test.md`
- Modify: `crates/divine-atbridge/tests/localnet_contract.rs`

- [ ] **Step 1: Extend the contract test with env override assertions**

```rust
#[test]
fn localnet_override_examples_target_local_services() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();
    let bridge_env = std::fs::read_to_string(repo_root.join("deploy/localnet/bridge.env.example")).unwrap();
    let gateway_env = std::fs::read_to_string(repo_root.join("deploy/localnet/handle-gateway.env.example")).unwrap();
    assert!(bridge_env.contains("PLC_DIRECTORY_URL=https://plc."));
    assert!(bridge_env.contains("HANDLE_DOMAIN=divine.test"));
    assert!(gateway_env.contains("ATPROTO_PROVISIONING_URL"));
}
```

- [ ] **Step 2: Run the env override test and verify it fails**

Run: `cargo test -p divine-atbridge localnet_override_examples_target_local_services -- --nocapture`
Expected: FAIL because the env example files do not exist yet.

- [ ] **Step 3: Add the localnet env examples**

Create [deploy/localnet/bridge.env.example](/Users/rabble/code/divine/divine-sky/deploy/localnet/bridge.env.example) with:
- `PDS_URL=https://pds.<tailnet>.ts.net`
- `PLC_DIRECTORY_URL=https://plc.<tailnet>.ts.net`
- `HANDLE_DOMAIN=divine.test`
- `RELAY_SOURCE_NAME=localnet-relay`

Create [deploy/localnet/handle-gateway.env.example](/Users/rabble/code/divine/divine-sky/deploy/localnet/handle-gateway.env.example) with:
- `ATPROTO_PROVISIONING_URL=http://127.0.0.1:3200/provision`
- `ATPROTO_KEYCAST_SYNC_URL`
- `ATPROTO_NAME_SERVER_SYNC_URL`
- `ATPROTO_NAME_SERVER_SYNC_TOKEN`

- [ ] **Step 4: Update the runbooks**

Update [docs/runbooks/dev-bootstrap.md](/Users/rabble/code/divine/divine-sky/docs/runbooks/dev-bootstrap.md) and [docs/runbooks/atproto-opt-in-smoke-test.md](/Users/rabble/code/divine/divine-sky/docs/runbooks/atproto-opt-in-smoke-test.md) so they explain:
- when to use the fast stack
- when to use the full localnet lab
- which sibling repos still matter for the user-facing opt-in flow
- how `divine.test` differs from the production `divine.video` handle domain

- [ ] **Step 5: Re-run the env override test**

Run: `cargo test -p divine-atbridge localnet_override_examples_target_local_services -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add deploy/localnet/bridge.env.example deploy/localnet/handle-gateway.env.example docs/runbooks/dev-bootstrap.md docs/runbooks/atproto-opt-in-smoke-test.md crates/divine-atbridge/tests/localnet_contract.rs
git commit -m "docs: add localnet bridge and gateway overrides"
```

### Task 6: Add Operator Scripts And A Repeatable Smoke Path

**Files:**
- Create: `scripts/localnet-up.sh`
- Create: `scripts/localnet-down.sh`
- Create: `scripts/localnet-smoke.sh`
- Create: `docs/runbooks/localnet-lab.md`
- Create: `docs/runbooks/localnet-provisioning-smoke-test.md`
- Modify: `crates/divine-atbridge/tests/localnet_contract.rs`

- [ ] **Step 1: Add failing assertions for scripts and runbooks**

```rust
#[test]
fn localnet_scripts_and_runbooks_exist() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();
    assert!(repo_root.join("scripts/localnet-up.sh").exists());
    assert!(repo_root.join("docs/runbooks/localnet-lab.md").exists());
}
```

- [ ] **Step 2: Run the new assertions and verify they fail**

Run: `cargo test -p divine-atbridge localnet_scripts_and_runbooks_exist -- --nocapture`
Expected: FAIL because the scripts and runbooks do not exist yet.

- [ ] **Step 3: Add the operator scripts**

Create:
- [scripts/localnet-up.sh](/Users/rabble/code/divine/divine-sky/scripts/localnet-up.sh) to start PLC, then PDS, then Jetstream, then DNS, and print the remaining manual Tailscale auth or cert commands
- [scripts/localnet-down.sh](/Users/rabble/code/divine/divine-sky/scripts/localnet-down.sh) to stop the same slices in reverse order
- [scripts/localnet-smoke.sh](/Users/rabble/code/divine/divine-sky/scripts/localnet-smoke.sh) to curl health endpoints and perform a minimal provisioning smoke sequence

- [ ] **Step 4: Add the operator runbooks**

Create [docs/runbooks/localnet-lab.md](/Users/rabble/code/divine/divine-sky/docs/runbooks/localnet-lab.md) and [docs/runbooks/localnet-provisioning-smoke-test.md](/Users/rabble/code/divine/divine-sky/docs/runbooks/localnet-provisioning-smoke-test.md) covering:
- required local dependencies
- Tailscale prerequisites
- expected hostnames
- how to create a `username.divine.test` mapping through the local handle-admin service
- how to point `divine-atbridge` and `divine-handle-gateway` at the lab
- cleanup and reset steps

- [ ] **Step 5: Validate shell syntax**

Run: `bash -n scripts/localnet-up.sh scripts/localnet-down.sh scripts/localnet-smoke.sh`
Expected: exit code `0`

- [ ] **Step 6: Re-run the script/runbook assertions**

Run: `cargo test -p divine-atbridge localnet_scripts_and_runbooks_exist -- --nocapture`
Expected: PASS

- [ ] **Step 7: Run the workspace verification**

Run: `bash scripts/test-workspace.sh`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add scripts/localnet-up.sh scripts/localnet-down.sh scripts/localnet-smoke.sh docs/runbooks/localnet-lab.md docs/runbooks/localnet-provisioning-smoke-test.md crates/divine-atbridge/tests/localnet_contract.rs
git commit -m "feat: add localnet lab operator flow"
```

## Success Criteria

- The repo has a documented `deploy/localnet/` profile for PLC, PDS, Jetstream, DNS, and handle admin.
- The fast local stack in [config/docker-compose.yml](/Users/rabble/code/divine/divine-sky/config/docker-compose.yml) remains intact and clearly documented as the default dev path.
- Bridge and handle-gateway can be pointed at the localnet lab using env files rather than code forks.
- Contract tests verify the presence and shape of the localnet profile without requiring Tailscale in CI.
- Operators have a repeatable smoke path for provisioning a `username.divine.test` identity through the isolated lab.

## Risks To Watch During Execution

- `rsky-pds` may behave differently from the official Bluesky PDS for account creation or Jetstream compatibility; keep the image override real, not hypothetical.
- Tailscale certificate generation is manual enough that broken docs will make the stack feel flaky even if the compose files are correct.
- A DNS admin service that writes CoreDNS state must avoid race conditions and partial-file writes.
- The plan intentionally avoids folding localnet concerns into production runtime code; resist shortcuts that blur those boundaries.
