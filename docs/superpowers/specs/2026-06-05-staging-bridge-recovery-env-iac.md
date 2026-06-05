# IaC handoff: wire recovery-key env into the staging atbridge

**Date:** 2026-06-05. **Target repo:** the IaC repo (NOT divine-sky). **For:** iac-coreconfig.

## Why
The rsky-native provisioning change (divine-sky branch `feat/atbridge-rsky-native-provisioning`)
makes the bridge pass an **offline PLC recovery key** to rsky on every `createAccount`, so every
new account's DID lists a cold recovery key first in `rotationKeys`. The bridge reads that key
from a new env var. **If the env var is absent the bridge omits the recovery key silently** —
accounts still provision and crossposting still works, but the DID has NO recovery key. So this
wiring is a launch-gating prerequisite, not a nice-to-have.

## Change (staging first)
Add **two** env vars to the `divine-atbridge` staging runtime (ExternalSecret keys + the
production/staging overlay env mapping, same pattern as the existing 10 keys):

| Env var | Value (staging) | Required? |
|---|---|---|
| `PLC_RECOVERY_ROTATION_DID_KEYS` | the staging recovery **public** `did:key` (see below) | **YES — make-or-break** |
| `ACCOUNT_EMAIL_DOMAIN` | `divine.video` | optional (code defaults to `divine.video`) |

`PLC_RECOVERY_ROTATION_DID_KEYS` is a comma-separated list of `did:key:` values; the bridge
validates each starts with `did:key:`, dedups, and caps at 4. For staging, a single key:

```
PLC_RECOVERY_ROTATION_DID_KEYS=did:key:zQ3shq8UUvhTbj8bzTyCnGkT6C7pFDVKAB23bd2JcsENaSgjk
```

This is the PUBLIC half — safe to put in the ExternalSecret/Secret Manager and in the pod env.
The corresponding PRIVATE half lives in `divine-atproto-plc-recovery-key-private-staging`
(Secret Manager, `dv-platform-staging`) and must NOT be wired into the cluster (break-glass only).

Backing GCP secret already exists: `divine-atproto-plc-recovery-key-did-staging`
(project `dv-platform-staging`) holds the public did:key — point the ExternalSecret at it, or
inline the value since it is public.

## Prod (later, when promoting)
Same two vars in the production overlay, using the PROD public did:key
`did:key:zQ3shqtkyxqEpU468PfA6nKHpFbKwGx6oaao6jEs5cpxerjv1`
(secret `divine-atproto-plc-recovery-key-did-production`, project `dv-platform-prod`).
Do NOT reuse the staging key in prod.

## Acceptance
After the overlay applies and the bridge pod restarts, `env | grep PLC_RECOVERY` inside the
pod shows the did:key. The divine-sky-side e2e gate then asserts the minted DID's
`rotationKeys[0]` equals this did:key (see
`docs/superpowers/plans/2026-06-05-rsky-native-provisioning.md` Task 5).
