# ATProto Identity Key Custody & Durability (launch gate)

**Date:** 2026-06-05. Answers: "how do we keep the identity keys safe and long-lasting?"

In the rsky-native provisioning model (Option B), **rsky-pds is the PLC authority for every Divine account** — one shared rotation key controls all identities. This concentrates risk, so key custody is a launch-blocking concern, not an afterthought.

## The two keys (very different stakes)

| Key | Role | If lost | If leaked |
|---|---|---|---|
| **PLC rotation key** (`PDS_PLC_ROTATION_KEY`) | Root of identity control for ALL accounts. Can rewrite any DID doc (PDS, signing key, tombstone). | **Permanent loss of identity control** for every account (PLC ops can't be re-signed) — unless a recovery key exists. | Attacker can hijack/redirect every account. |
| **Repo signing key** (`PDS_REPO_SIGNING_KEY`) | Signs repo commits. | Recoverable: rotate via a PLC op (needs rotation key), re-sign going forward. | Lower stakes; rotate it out. |
| JWT secret/key | Session tokens. | Sessions invalidated; re-issue. Not identity. | Rotate. |

## What's verified (2026-06-05, staging)

- ✅ Keys live in **GCP Secret Manager** (`dv-platform-staging`), Google-managed encryption, with version history (v1+v2 present).
- ✅ **Access is least-privilege at the workload level**: no secret-level bindings (inherits project IAM); project `secretAccessor` = only `argocd-staging` + `external-secrets-staging` service accounts; `secretmanager.admin` = only `github-actions-staging` + `terraform-staging`. No human/broad access to the rotation key. Good.
- ✅ **rsky supports a recovery rotation key natively** — `format_did_and_plc_op` (create_account.rs) takes an optional `recovery_key` and lists it FIRST (higher priority) in the DID's `rotation_keys`, before rsky's own key. So a cold recovery key can be baked into every genesis op with **no rsky code change** — the bridge just passes `recovery_key` in the createAccount call.

## Gaps / required before "long-lasting" is true

1. **[BLOCKER, decide before launch] Recovery rotation key in every DID.** rsky supports it; we must (a) generate a recovery keypair, (b) keep its PRIVATE key **cold/offline** — NOT in the same SM project the cluster reads (else it's not a real recovery path), (c) pass its `did:key` (public) as `recovery_key` on every createAccount. Must be set AT CREATION — cannot be cheaply retrofitted to existing DIDs (one PLC op each). The existing 9 staging repos do NOT have one (single rotation key) — acceptable for staging throwaways, NOT for prod.
2. **[verify] Offline backup of the rotation key.** SM is durable but a deleted/locked project or a bad rotation is survivable only with a separate, access-controlled, offline backup. Confirm one exists for prod.
3. **[verify] Data-access audit logging** on Secret Manager access (could not confirm `auditConfigs` from here) — so rotation-key reads are logged.
4. **[missing] Rotation runbook.** A tested procedure to rotate the operational rotation key (using the recovery key) across all DIDs if compromise is suspected.
5. **Prod uses its OWN keys.** Do NOT reuse staging keys in prod; generate fresh prod rotation/signing/recovery keys (this is part of the prod-secret creation already tracked).

## Scale & cost: do NOT store per-DID keys as individual GCP secrets

Option B (rsky shared keys) stores **no per-account secrets** — only rsky's ~5 shared
keys + 1 offline recovery key ≈ 6 secrets total. Secret Manager cost is **O(1)**:
~$0.36/mo flat regardless of user count (SM = $0.06 / active version / location / month).
Project currently has 77 secrets.

The rejected per-DID-secret model is both expensive AND quota-impossible:
- **Quota:** SM caps secrets per project (default ~10k–25k). Per-DID secrets → provisioning
  HARD-FAILS past that — a functional wall, not just a bill.
- **Cost:** ~$60/mo at 1k users, ~$600 at 10k, ~$6,000 at 100k (storage alone), plus
  access-op cost (ESO reconcile re-reading 100k secrets ≈ tens of millions of reads/mo)
  × replicas × environments. ESO's reconcile loop degrades badly first.
- **DIDs are PUBLIC** — the `did:plc:...` identifier is not secret; it belongs in Postgres
  (`account_links`) and the PLC directory. Never put a DID string in Secret Manager.

If per-user private keys are ever needed (NOT the case in Option B), the org's PROVEN
pattern is keycast's: **encrypted blobs in Postgres under a single GCP KMS master key**
(keycast `core/src/encryption/gcp_key_manager.rs`, KMS key ring `keycast-keys` / master
`master-key`; user keys in `stored_keys.secret_key` / `encrypted_secret_key bytea`).
N keys in the DB, ONE KMS master key — O(1) secret cost. Use KMS (purpose-built for
encrypting unlimited data under one key), NOT Secret Manager, for per-user key material.

VERIFIED 2026-06-05: there is NO per-user-secret sprawl in any project — prod has 79
secrets total, keycast 10 (all server/infra level, incl. one `keycast-server-nsec`).
So Divine is NOT silently paying for tens of thousands of per-user secrets; the feared
"$1000/mo" pattern does not exist. keycast already does per-user keys the right way
(KMS + Postgres).

(List-price math; Divine may have committed-use discounts. The O(1)-vs-O(users) and the
quota wall hold regardless.)

## Recovery key status (2026-06-05)
- **Staging recovery key CREATED.** secp256k1 keypair, project's exact did:key encoding
  (`0xe701` multicodec + compressed pubkey, base58btc). Private →
  `divine-atproto-plc-recovery-key-private-staging`; public did:key →
  `divine-atproto-plc-recovery-key-did-staging` =
  `did:key:zQ3shq8UUvhTbj8bzTyCnGkT6C7pFDVKAB23bd2JcsENaSgjk`. Throwaway generator, deleted;
  private key never written to disk.
- **⚠️ KNOWN GAP (staging, accepted):** the private recovery key inherits project IAM →
  `external-secrets` + `argocd` SAs can read it, the SAME accounts that read the operational
  rotation key. So it's not yet more isolated than the key it recovers → minimal real recovery
  value. Fine for resettable staging; NOT for prod.
- **PROD GATE:** isolate the prod private recovery key from the cluster — either keep it
  cold/offline outside the cluster-readable SM project (cluster only needs the PUBLIC did:key
  via `PLC_RECOVERY_ROTATION_DID_KEYS`), or a secret-level IAM policy excluding app/ESO/argocd
  SAs (break-glass only). Also still pending: offline BACKUP of the operational rotation key.


- **Prod recovery key CREATED 2026-06-05** (fresh, not reused from staging): private →
  `divine-atproto-plc-recovery-key-private-production` (SM has ONE version, v1 — almost
  certainly the ONLY copy; deleting it destroys the key); public did:key →
  `divine-atproto-plc-recovery-key-did-production` =
  `did:key:zQ3shqtkyxqEpU468PfA6nKHpFbKwGx6oaao6jEs5cpxerjv1`.
- **❌ CMEK WAS THE WRONG TOOL (corrected 2026-06-05).** I CMEK-encrypted the private secret
  with KMS key `plc-recovery-cmek` (keyring `app-keys-production`, us-central1) and granted
  decrypt only to the SM service agent. This does NOT isolate any caller: **Secret Manager
  CMEK is transparent to callers** — the SM service agent performs the decrypt, and a caller
  needs ONLY `roles/secretmanager.secretAccessor` (zero KMS permission) to read the plaintext.
  So CMEK gave a kill-switch (disable the KMS key → nobody reads the secret), NOT caller
  isolation. Corollary: `keycast-migration`'s project-level KMS grant is irrelevant to secret
  access — do NOT touch it (would break keycast's own master-key access for no benefit).
- **⚠️ PROD ISOLATION STILL NOT ACHIEVED. The ONLY real lever is `secretAccessor`, and it's
  project-level.** `argocd-production`, `external-secrets-production`, and `keycast-migration`
  all hold project-level `secretAccessor` → they can read the private recovery secret (CMEK or
  not). A secret-level allow can't subtract a project-level grant; the deny policy that would
  is org-level (`denypolicies.create denied` for rabble@divine.video). Therefore the correct
  fix is **(c): the private half must NOT live in dv-platform-prod at all.** The cluster only
  ever consumes the PUBLIC did:key (via `PLC_RECOVERY_ROTATION_DID_KEYS`, added in Option B
  Task 1); the bridge never reads the private half — it's break-glass-only.
- **BLOCKING SAFETY GATE before removing the private secret:** SM v1 is likely the only copy.
  Removing it from the project requires FIRST securing a durable copy in the chosen cold
  destination. Do NOT delete first. Do NOT print the private value into any transcript/log.
  Destination is an OWNER decision (separate isolated GCP project vs. offline/cold human
  custody) — see launch-gate note. This is a PROD-PROMOTION gate, not a launch blocker: no
  prod accounts exist yet and Option B isn't deployed, so the private key currently has zero
  live consumers. Sequence it AFTER staging Option B + staging e2e are proven.

## Bottom line
Day-to-day safety is reasonable (SM + tight workload IAM). "Long-lasting" is **not yet true**
until the recovery key is in every DID (config/wiring, Task 1–2 of the Option-B plan),
the prod recovery private key is isolated from the cluster (gate above), and the operational
rotation key has an offline backup. rsky natively supports the recovery key, so it's wiring,
not an rsky change — but it MUST be set before the first PROD account is created.
