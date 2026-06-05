# Rollout Chunk C — Fix the divine-router Fastly KV Store Mismatch

> **Parent plan:** `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk C).
> **Editability:** `cross-repo-spec-only`. The code changes in this chunk land in the **sibling repo** `/Users/rabble/code/divine/divine-router`. This sub-plan is the divine-sky-side spec; **do not** commit edits to divine-router from a divine-sky agent except the two-line `fastly.toml` change described here, executed in the divine-router working tree by whoever owns that repo. The verification commands are run against the live Fastly service.
>
> **For agentic workers:** execute task-by-task; check each `- [ ]` as you go. Every command below is real and copy-pasteable. No placeholders except the live-discovered `<ready-user>` / `<not-ready-user>` handles in Task C3, which Step C3.1 tells you how to obtain.

---

## Problem statement (verified 2026-05-30)

`divine-router` is a Fastly Compute@Edge service that resolves `username.divine.video/.well-known/atproto-did` by reading a Fastly KV store. There is a **store-name binding mismatch** plus a **missing `store_id`** that prevents `KVStore::open` from resolving the correct store at deploy/runtime:

| Layer | What it says today | File:line |
|---|---|---|
| **Code** (the runtime `KVStore::open` arg) | `const KV_STORE_NAME: &str = "divine-names";` | `/Users/rabble/code/divine/divine-router/src/main.rs:15` |
| **Code** (lookup key format) | `let key = format!("user:{}", username);` | `/Users/rabble/code/divine/divine-router/src/main.rs:396` |
| **Config** (`fastly.toml` runtime store binding) | `[setup.kv_stores.usernames]` with **no `store_id`** | `/Users/rabble/code/divine/divine-router/fastly.toml:65` |
| **Config** (`fastly.toml` local-dev store binding) | `[local_server.kv_stores.usernames]` → `kv_usernames.json` | `/Users/rabble/code/divine/divine-router/fastly.toml:32-33` |
| **Writer** (the producer of the KV data) | writes to `FASTLY_STORE_ID = "gclbp6suv4bjnqpctp2b7n"`, key `user:${username}` | `/Users/rabble/code/divine/divine-name-server/wrangler.toml:29` and `/Users/rabble/code/divine/divine-name-server/src/utils/fastly-sync.ts:70-71` |

**Why this is a runtime bug:** the `[setup.kv_stores.<NAME>]` key in `fastly.toml` is a deploy-time *alias* that `KVStore::open("<NAME>")` resolves by name. The code opens `"divine-names"` but the config only declares a binding named `"usernames"`, so `KVStore::open("divine-names")` returns `Ok(None)` → `lookup_username` returns `None` → every `/.well-known/atproto-did` request (and every profile lookup) falls through to **404**, even for ready users. The missing `store_id` separately means the binding isn't pinned to the physical store the writer populates.

## Canonical decision (this is the spec)

Two **separate** canonical facts. State both in the commit/PR description:

1. **Binding name → `divine-names`** (align config to code, **not** code to config). The code constant is the deliberate source of truth: commit `74d5b7c` ("fix: use correct KV store and key format for username lookup", Daniel Cadenas, 2026-02-03) changed the constant **from `usernames` to `divine-names`** and changed the key from bare `username` to `user:{username}` — the author found the live store and fixed the reader, but never updated `fastly.toml`. The README (`/Users/rabble/code/divine/divine-router/README.md:60`, "stored in the `usernames` KV store") is also stale prose, not authority. **Do not** revert the code to `usernames`.

2. **`store_id` → `gclbp6suv4bjnqpctp2b7n`**, sourced from the writer's config (`divine-name-server/wrangler.toml:29` `FASTLY_STORE_ID`). The decisive constraint is **the reader must point at the store the writer writes to** — that, not the name, is what makes reads return data.

This decision is robust regardless of what the *deployed* router service's KV link is named today:
- If the deployed link is currently `usernames` → the router is broken right now; renaming the binding + adding `store_id` fixes it.
- If the deployed link is already `divine-names` → `fastly.toml` is merely stale and out of sync with reality; this edit makes the repo match prod.

Either way, the same two-line edit resolves it. **No `src/main.rs` change is required.**

---

## Task C1: Confirm the writer/reader store contract (read-only, no edits)

- [ ] **Step C1.1: Re-confirm the code constant and key format**

```bash
grep -n 'KV_STORE_NAME\|format!("user:' /Users/rabble/code/divine/divine-router/src/main.rs
```
Expected output:
```
15:const KV_STORE_NAME: &str = "divine-names";
396:    let key = format!("user:{}", username);
```

- [ ] **Step C1.2: Re-confirm the writer's store_id and key format**

```bash
grep -n 'FASTLY_STORE_ID' /Users/rabble/code/divine/divine-name-server/wrangler.toml
grep -n 'user:\${username}\|user:{username}' /Users/rabble/code/divine/divine-name-server/src/utils/fastly-sync.ts
```
Expected:
```
29:FASTLY_STORE_ID = "gclbp6suv4bjnqpctp2b7n"
...
70:  const kvKey = `user:${username}`
```
The key format already matches the reader (`user:{username}`). Good — this chunk is **only** the store binding, not the key.

- [ ] **Step C1.3: Re-confirm the broken `fastly.toml` blocks**

```bash
grep -n 'kv_stores\|store_id\|usernames\|divine-names' /Users/rabble/code/divine/divine-router/fastly.toml
```
Expected (the bug): two `kv_stores.usernames` blocks, **no** `store_id`, **no** `divine-names`:
```
30:  [local_server.kv_stores]
32:    [local_server.kv_stores.usernames]
33:      file = "kv_usernames.json"
63:  [setup.kv_stores]
65:    [setup.kv_stores.usernames]
```

---

## Task C2: Apply the two-line `fastly.toml` fix (in the divine-router repo)

> Performed in the divine-router working tree by the divine-router owner. The current `fastly.toml` ends at line 65 (`[setup.kv_stores.usernames]` with no body) — the `store_id` line must be added.

**File:** `/Users/rabble/code/divine/divine-router/fastly.toml`

- [ ] **Step C2.1: Rename the `local_server` KV binding** so local dev resolves the same name the code opens.

Change:
```toml
  [local_server.kv_stores]

    [local_server.kv_stores.usernames]
      file = "kv_usernames.json"
      format = "json"
```
to:
```toml
  [local_server.kv_stores]

    [local_server.kv_stores.divine-names]
      file = "kv_usernames.json"
      format = "json"
```

- [ ] **Step C2.2: Rename the `setup` KV binding AND add the `store_id`.**

Change:
```toml
  [setup.kv_stores]

    [setup.kv_stores.usernames]
```
to:
```toml
  [setup.kv_stores]

    [setup.kv_stores.divine-names]
      store_id = "gclbp6suv4bjnqpctp2b7n"
```

- [ ] **Step C2.3: Confirm the edit**

```bash
grep -n 'kv_stores\|store_id\|divine-names\|usernames' /Users/rabble/code/divine/divine-router/fastly.toml
```
Expected:
```
30:  [local_server.kv_stores]
32:    [local_server.kv_stores.divine-names]
...
63:  [setup.kv_stores]
65:    [setup.kv_stores.divine-names]
66:      store_id = "gclbp6suv4bjnqpctp2b7n"
```
No remaining `usernames` under `kv_stores`. (The string `usernames` may still appear in `kv_usernames.json` filename — that's fine.)

- [ ] **Step C2.4: Build/test still green (the manifest test reads `fastly.toml`)**

```bash
cd /Users/rabble/code/divine/divine-router && cargo test
```
Expected: all tests pass. (The `test_fastly_manifest_defines_runtime_backends` test only asserts backends, not KV stores, so the rename won't break it — but run the suite to be sure nothing else parses `fastly.toml`.)

---

## Task C3: Verify against the live Fastly store and service

> **Before publishing**, confirm the router's KV link and the writer's store are the **same physical store**. A rename alone won't fix reads if the router service is linked to a *different* `store_id`. This is the one inferred fact in the canonical decision — verify it, don't assume it.

**Prereq:** `fastly` CLI authenticated, and `$FASTLY_API_TOKEN` exported (same token family the name-server uses to write).

- [ ] **Step C3.1: Discover real ready / not-ready users (do NOT hardcode `rabble`)**

The `rabble` entry in `kv_usernames.json` is a **fake local fixture** (`did:plc:raaaaab1e000000000000000`). Get real handles from the name-server D1 database:

```bash
cd /Users/rabble/code/divine/divine-name-server
npx wrangler d1 execute divine-name-server-db --remote \
  --command "SELECT name FROM usernames WHERE atproto_state='ready' AND atproto_did IS NOT NULL LIMIT 1;"
npx wrangler d1 execute divine-name-server-db --remote \
  --command "SELECT name FROM usernames WHERE status='active' AND (atproto_state IS NULL OR atproto_state != 'ready') LIMIT 1;"
```
Record the first result as `<ready-user>` and the second as `<not-ready-user>`.

- [ ] **Step C3.2: Confirm the physical store exists and holds `user:*` keys**

```bash
fastly kv-store list --token "$FASTLY_API_TOKEN" | grep -i 'gclbp6suv4bjnqpctp2b7n'
fastly kv-store-entry list --store-id gclbp6suv4bjnqpctp2b7n --token "$FASTLY_API_TOKEN" | grep '^user:' | head
```
Expected: the store id `gclbp6suv4bjnqpctp2b7n` is listed, and entries like `user:<ready-user>` appear. If `user:*` keys are absent, the writer hasn't synced — coordinate with Chunk F (edge deploy order: name-server writes state first, then router publishes) before continuing.

- [ ] **Step C3.3: Confirm the router service will resolve the binding to that same store**

After publish (next step) the resource link is attached to the service. To check *before* publish whether the service already has a KV resource link and to which store:
```bash
fastly resource-link list --service-id 76fTayX6mBKa8faLeZ1fet --version active --token "$FASTLY_API_TOKEN"
```
If a KV resource link exists pointing at a **different** store_id than `gclbp6suv4bjnqpctp2b7n`, **stop**: a name rename won't redirect reads. Re-point the link (or the `store_id` in `fastly.toml`) to the store the writer populates. (This is the top risk for this chunk.)

- [ ] **Step C3.4: Publish and purge**

```bash
cd /Users/rabble/code/divine/divine-router && fastly compute publish --non-interactive && fastly purge --all
```
Expected: build succeeds, a new service version is activated, the KV store `divine-names` (store_id `gclbp6suv4bjnqpctp2b7n`) is shown as a linked resource in the publish output, cache purged.

- [ ] **Step C3.5: Curl the three cases**

```bash
# Case 1: ready user -> 200 + the DID body
curl -fsS "https://<ready-user>.divine.video/.well-known/atproto-did"
# Expected: a single line like  did:plc:xxxxxxxxxxxxxxxxxxxxxxxx  (no quotes, text/plain), HTTP 200

# Case 2: active but not ready -> 404, empty body
curl -s -o /dev/null -w '%{http_code}\n' "https://<not-ready-user>.divine.video/.well-known/atproto-did"
# Expected: 404

# Case 3: nonexistent user -> 404
curl -s -o /dev/null -w '%{http_code}\n' "https://this-handle-does-not-exist-zzz.divine.video/.well-known/atproto-did"
# Expected: 404
```

These three cases map exactly to the gate in `handle_atproto_did` (`src/main.rs:336-355`): it returns the DID only when `status == "active" && atproto_state == Some("ready") && atproto_did.is_some()`, otherwise 404 with an empty body.

- [ ] **Step C3.6 (sanity): confirm a profile/NIP-05 read also works** (proves the store binding, not just the DID path)

```bash
curl -fsS "https://<ready-user>.divine.video/.well-known/nostr.json?name=_" | jq '.names'
```
Expected: `{"_": "<hex-pubkey>"}` — non-empty, proving `lookup_username` now resolves through the `divine-names` binding.

---

## Task C4: Commit (in the divine-router repo)

- [ ] **Step C4.1: Stage and commit only `fastly.toml`**

```bash
cd /Users/rabble/code/divine/divine-router
git checkout -b fix/router-kv-store-binding   # current HEAD is detached at origin/main
git add fastly.toml
git commit -m "fix: bind router KV store to divine-names with prod store_id

KVStore::open opens \"divine-names\" (src/main.rs:15) but fastly.toml
declared a \"usernames\" binding with no store_id, so the store never
resolved and every atproto-did/profile lookup 404'd. Align the config
to the code (canonical per 74d5b7c) and pin store_id gclbp6suv4bjnqpctp2b7n,
the store the name-server writer populates.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step C4.2 (secondary, optional): fix the stale local fixture.** `kv_usernames.json` keys are **bare** (`"rabble"`, `"alice"`) but the code looks up `user:{username}`, so `fastly compute serve` stays broken even after the rename. If local-dev parity matters, re-key the fixture entries to `user:rabble`, `user:alice`, etc. This does **not** affect the live curl verification above (which runs against the real Fastly store, not the local fixture) — keep it out of the critical-path commit unless local serve is needed.

- [ ] **Step C4.3: Open PR** (Chunk F adds router CI; until then this deploys manually via Task C3.4).

```bash
cd /Users/rabble/code/divine/divine-router && gh pr create --fill
```

---

## Done criteria

- [ ] `fastly.toml` declares exactly one KV binding, named `divine-names`, with `store_id = "gclbp6suv4bjnqpctp2b7n"`, in both `[local_server.kv_stores]` and `[setup.kv_stores]`.
- [ ] `cargo test` green in divine-router.
- [ ] Live: `<ready-user>.divine.video/.well-known/atproto-did` → 200 + DID; `<not-ready-user>` → 404; nonexistent → 404.
- [ ] Live: `<ready-user>.divine.video/.well-known/nostr.json?name=_` returns a non-empty `names` map (proves the binding resolves at all).
- [ ] The router service's KV resource link points at store_id `gclbp6suv4bjnqpctp2b7n` (same store the writer writes).

## Risks

- **Top risk — wrong physical store.** If the deployed router service's KV resource link already points at a *different* store_id than the writer's `gclbp6suv4bjnqpctp2b7n`, the name rename alone won't fix reads (Step C3.3 catches this). The reader/writer store_id must match; that is the load-bearing invariant, not the binding name.
- **Writer hasn't synced.** If `user:*` keys are absent from the store (Step C3.2), the 404s are a data problem, not a binding problem — sequence after the name-server sync (Chunk F deploy order).
- **Disable-flow coupling (Chunk H).** The 404-for-non-ready behavior verified here is also the user-facing safety contract: disabling an account must flip `atproto_state` away from `ready` so the router stops resolving the DID. Cross-check with Chunk H Step 3.
