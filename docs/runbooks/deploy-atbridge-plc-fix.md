# Deploy the ATProto PLC-fix atbridge image (Gate 1)

Turnkey build+deploy for the provisioning fixes in PR #12. This is the one step
that unblocks all e2e verification — staging atbridge currently runs an image
WITHOUT the PLC fixes, so provisioning fails until this lands.

**Prereq:** registry write access to `dv-platform-staging` and `gcloud`/`docker`
auth. Run from the `divine-sky` repo root.

## 1. Get the image tag (git short-SHA — matches the existing tagging convention)

After PR #12 merges to `main`, build from the merge commit:
```bash
git checkout main && git pull
SHA="$(git rev-parse --short HEAD)"
echo "building atbridge $SHA"
```
(Existing staging tag is a short-SHA, e.g. `3ba324c`. Pre-merge, you may build
from the branch tip instead: `git rev-parse --short origin/atproto-production-rollout`.)

## 2. Build + push the staging image

`Dockerfile.atbridge` is self-contained (multi-stage Rust build, copies
`crates/` + `migrations/`). Either Docker or Cloud Build:

```bash
# Option A: local docker (needs `gcloud auth configure-docker us-central1-docker.pkg.dev`)
IMG="us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/divine-atbridge:${SHA}"
docker build -f Dockerfile.atbridge -t "$IMG" .
docker push "$IMG"

# Option B: Cloud Build (no local docker)
gcloud builds submit --project=dv-platform-staging \
  --tag "us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/divine-atbridge:${SHA}" \
  --substitutions=_DOCKERFILE=Dockerfile.atbridge .   # adjust if your cloudbuild expects a config
```

## 3. Bump the staging overlay (in divine-iac-coreconfig)

Edit `k8s/applications/divine-atbridge/overlays/staging/kustomization.yaml`:
```yaml
images:
  - name: divine-atbridge
    newName: us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/divine-atbridge
    newTag: <SHA>          # <- was 3ba324c
```
Commit + push + merge; ArgoCD syncs the `divine-atbridge` Application (staging).

## 4. Verify the deploy

```bash
SC=connectgateway_dv-platform-staging_us-central1_gke-staging-membership
kubectl --context $SC rollout status deploy/divine-atbridge -n sky
kubectl --context $SC get deploy divine-atbridge -n sky -o jsonpath='{..image}{"\n"}'   # shows :<SHA>
# the self-migration must run cleanly on boot:
kubectl --context $SC logs deploy/divine-atbridge -n sky | grep -iE 'migration|column .* does not exist|panic'
```
Expected: image tag is `<SHA>`; log shows `bridge database migrations applied`; no schema error.

## 5. Hand back for Gate 2

Once the new pod is up, the e2e crosspost test (opt-in → provision → post →
verify on Bluesky) can run — see `2026-06-03-atproto-production-promotion.md`
Chunk F. Expect to hit Wall 2/3 (createAccount auth + the deactivated-account /
missing-activateAccount bug) on first provision; those are diagnosed live.

## Prod (later, after staging proves out)
Same steps with `dv-platform-prod` / `containers-production`, and pin the prod
overlay `newTag` off `latest` to the same `<SHA>` (Chunk C). Do NOT promote to
prod until staging shows a real crosspost on Bluesky.
