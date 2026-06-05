# ATProto crossposting — production promotion + test plan

**Date:** 2026-06-05. Staging is deployed + verified (provisioning rsky-native, recovery
key first in DID, REST live ingest). The apps (divine-mobile/desktop) point at
**production keycast** (`https://login.divine.video`), so users can't turn it on until prod
is promoted. This is the checklist, grounded in the **live prod state** read from the pods.

## Verified prod state (dv-platform-prod, namespace `sky`)
Pods running (2d10h): `divine-atbridge`, `divine-handle-gateway` (×2), `rsky-pds`.

**Already correct in prod (no change needed):**
- atbridge `HANDLE_DOMAIN=divine.video`, `PDS_URL=https://pds.divine.video`,
  `PLC_DIRECTORY_URL=https://plc.directory`, `VIDEO_SERVICE_ENABLED=true`,
  `RELAY_URL=wss://relay.divine.video`, `RELAY_REST_URL` unset → new code derives
  `https://relay.divine.video/api` (correct), `ACCOUNT_EMAIL_DOMAIN` unset → defaults `divine.video`.
- rsky-pds `PDS_HOSTNAME=pds.divine.video`, `PDS_SERVICE_HANDLE_DOMAINS=.divine.video`
  (single-label handles OK), `PDS_INVITE_REQUIRED=true` (new bridge mints invites),
  `PDS_PLC_ROTATION_KEY...` set, `PDS_ADMIN_PASSWORD` set.

## GAPS to fix before prod can crosspost
1. **Images on `:latest` = pre-fix code.** prod `divine-atbridge` runs
   `containers-production/divine-atbridge:latest` — no rsky-native provisioning, no Wall-4
   session auth, no REST ingest. MUST build the current `main` and pin the overlay.
2. **`PDS_AUTH_TOKEN` ≠ rsky `PDS_ADMIN_PASSWORD`** (verified: sha256 mismatch). This is the
   staging "BadAuth on createAccount" wall. atbridge's `PDS_AUTH_TOKEN` must equal rsky-pds's
   `PDS_ADMIN_PASSWORD` (the Basic-auth admin pw used for createInviteCode + createAccount).
3. **`PLC_RECOVERY_ROTATION_DID_KEYS` UNSET.** Set to the prod recovery public did:key
   `did:key:zQ3shqtkyxqEpU468PfA6nKHpFbKwGx6oaao6jEs5cpxerjv1` — else accounts mint with NO
   recovery key (silent; permanent per-DID).

## Deploy steps
**A. Build prod image (divine-sky side).**
```
SHA=$(git rev-parse --short origin/main)   # current = the REST-ingest build
gcloud builds submit --project=dv-platform-prod \
  --config=<cloudbuild docker -f Dockerfile.atbridge> \
  -> us-central1-docker.pkg.dev/dv-platform-prod/containers-production/divine-atbridge:$SHA
```
(handle-gateway does NOT need a rebuild — the provision contract {nostr_pubkey,handle} is
unchanged; all fixes are in atbridge. Rebuild only if you want parity.)

**B. Secrets (dv-platform-prod Secret Manager, applied via IaC/ESO — iac-coreconfig).**
- Set the atbridge `PDS_AUTH_TOKEN` backing secret = rsky-pds `PDS_ADMIN_PASSWORD` value.
- Set the atbridge `PLC_RECOVERY_ROTATION_DID_KEYS` backing secret =
  `did:key:zQ3shqtkyxqEpU468PfA6nKHpFbKwGx6oaao6jEs5cpxerjv1`.

**C. Pin prod overlay (iac-coreconfig).** `divine-atbridge` production overlay
`newTag: <SHA>` (off `:latest`). ArgoCD sync. The new image self-applies bridge DB
migrations (incl. 006 session columns) on boot — idempotent.

**D. Recovery-key custody GATE (decide before real users).** The prod *private* recovery key
(`divine-atproto-plc-recovery-key-private-production`) is still in the cluster-readable SM
project. Per `atproto-identity-key-custody.md`, move the private half cold/offline (cluster
only needs the public did:key in step B). Real prod DIDs minting under this key make it the
identity root for all prod accounts.

## Test plan (after deploy)
1. Pod healthy on `:<SHA>`; logs: `bridge database migrations applied`,
   `starting REST live-ingest poll loop rest_url=https://relay.divine.video/api`.
2. Opt in a test account: from the app (Bluesky Publishing toggle) OR directly
   `POST /provision` on the atbridge internal API (port-forward :8080, Bearer
   `ATPROTO_PROVISIONING_TOKEN`, body `{nostr_pubkey, handle:"<single-label>.divine.video"}`).
3. **PASS/FAIL GATE:** `https://plc.directory/<did>/data` → `rotationKeys[0]` ==
   `did:key:zQ3shqtkyxqEpU468PfA6nKHpFbKwGx6oaao6jEs5cpxerjv1`, `[1]` == rsky's key;
   `com.atproto.sync.getRepoStatus?did=<did>` → `active:true`.
4. Post a real NIP-71 video as that account → confirm REST poller logs
   `REST ingest enqueued new live events` → it appears AND plays on Bluesky.

## Separate issues found (not blocking the above, but real)
- **ROTATE leaked secrets:** during diagnosis the prod `PDS_AUTH_TOKEN`,
  `ATPROTO_PROVISIONING_TOKEN`, and bridge `DATABASE_URL` password were printed to a session
  transcript. Rotate them.
- **keycast prod Redis storm:** keycast (`identity` ns) `cluster_hashring` coordinator logging
  `Heartbeat failed: Redis error: broken pipe` ~71k times — investigate; may degrade keycast.
- **divine-mobile field mismatch:** client parses `crosspost_enabled` but keycast returns
  `enabled` (`AtprotoStatusResponse`); fix or status/toggle will read wrong. Also confirm
  keycast exposes `GET /api/account/{pubkey}/crosspost` (mobile calls it for status).
- **divine-web:** no opt-in UI at all (separate build).
