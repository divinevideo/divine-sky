# IaC handoff: promote ATProto crossposting to PRODUCTION

**Date:** 2026-06-05. **Target repo:** divine-iac-coreconfig. **For:** iac-coreconfig.
**Context:** staging is verified; apps point at prod keycast (login.divine.video). Prod
`divine-atbridge` runs `:latest` (pre-fix) and has 2 config gaps. Image built: see step 1.

## 1. Pin the prod atbridge image (off `:latest`)
Image built + pushed: `us-central1-docker.pkg.dev/dv-platform-prod/containers-production/divine-atbridge:121dae8`
(the rsky-native provisioning + REST live-ingest build).

In the **production** overlay `k8s/applications/divine-atbridge/overlays/production/kustomization.yaml`:
```yaml
images:
  - name: divine-atbridge
    newName: us-central1-docker.pkg.dev/dv-platform-prod/containers-production/divine-atbridge
    newTag: 121dae8        # was :latest
```

## 2. Fix `PDS_AUTH_TOKEN` (BadAuth wall) — ✅ DONE, no iac action needed
The backing SM secret `divine-atbridge-pds-auth-token-production` was updated to the rsky-pds
admin password value (added as version 2; sha256 verified equal to rsky's `PDS_ADMIN_PASSWORD`).
The existing ExternalSecret mapping is unchanged, so ESO will sync the corrected value on the
next reconcile; pods pick it up on the rollout triggered by step 1. No iac change required here.
(Optional future hardening: repoint this ExternalSecret at `rsky-pds-admin-password-production`
so the two can never drift.)

## 3. Wire `PLC_RECOVERY_ROTATION_DID_KEYS` (currently UNSET → recovery key silently absent)
Add env var to the prod atbridge runtime, sourced from the existing public-key secret:
- env `PLC_RECOVERY_ROTATION_DID_KEYS` → SM secret
  **`divine-atproto-plc-recovery-key-did-production`** (already holds
  `did:key:zQ3shqtkyxqEpU468PfA6nKHpFbKwGx6oaao6jEs5cpxerjv1`, the PUBLIC half — safe in cluster).
- `ACCOUNT_EMAIL_DOMAIN` is optional (code defaults to `divine.video`).
- Do NOT wire the private secret (`...-private-production`) — break-glass only.

## After apply
ArgoCD sync → new rollout. The new image self-applies bridge DB migrations (incl. 006 session
columns) on boot (idempotent). Then ping divine-sky to run the prod e2e:
- pod healthy on `:121dae8`; logs `starting REST live-ingest poll loop rest_url=https://relay.divine.video/api`.
- opt in a test account → `plc.directory/<did>/data` `rotationKeys[0]` ==
  `did:key:zQ3shqtkyxqEpU468PfA6nKHpFbKwGx6oaao6jEs5cpxerjv1`, `active:true` → post video → plays on Bluesky.

## Notes
- Prod already-correct (no change): HANDLE_DOMAIN=divine.video, PDS_URL, PLC_DIRECTORY_URL,
  VIDEO_SERVICE_ENABLED=true, rsky PDS_SERVICE_HANDLE_DOMAINS=.divine.video, PDS_INVITE_REQUIRED=true.
- Separate (non-blocking): keycast prod Redis broken-pipe storm (identity ns); divine-mobile
  reads `crosspost_enabled` but keycast returns `enabled`.
