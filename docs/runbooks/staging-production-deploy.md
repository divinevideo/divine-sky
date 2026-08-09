# Staging And Production Deploy

## Purpose

`divine-sky` staging and production deploys are owned by `../divine-iac-coreconfig`. This repository defines the runtime contract; coreconfig owns the Kubernetes manifests, secrets, namespace, routes, and ArgoCD registration.

## Runtime Contract

All four runnable services deploy into the shared `sky` namespace:

- `divine-atbridge`
  - internal worker
  - no public route
  - singleton deployment initially
- `divine-handle-gateway`
  - internal HTTP service
  - no public route
  - reachable only by cluster DNS
- `divine-feedgen`
  - public HTTP/XRPC service
  - exposed through Gateway API and external DNS
- `divine-labeler`
  - public ATProto label-query service
  - exposed through Gateway API and external DNS

Only `divine-feedgen` and `divine-labeler` should be visible outside the cluster.

## Hostnames

Use these public hostnames in staging and production:

- staging feed: `feed.staging.dvines.org`
- production feed: `feed.divine.video`
- staging labeler: `labeler.staging.dvines.org`
- production labeler: `labeler.divine.video`

## Coreconfig Layout

In `../divine-iac-coreconfig`, each service should have:

- `k8s/applications/<service>/base/`
- `k8s/applications/<service>/overlays/staging/`
- `k8s/applications/<service>/overlays/production/`
- `k8s/argocd/apps/<service>.yaml`

The `sky` namespace should be declared under `k8s/cluster-config/namespaces/`.

## Image Publishing

`.github/workflows/docker.yml` publishes the four runnable service images:

- `divine-atbridge`
- `divine-handle-gateway`
- `divine-feedgen`
- `divine-labeler`

Pull requests build the images but do not push them. Pushes to `main` wait for
the Rust workflow to pass for the same commit before publishing to both
`containers-staging` and `containers-production`. Manual republishes use the
Docker workflow's `workflow_dispatch` input, which takes a branch, tag, or SHA
that resolves to a commit already on `main`. The Rust gate only accepts a Rust
run from a `push` event, and `rust.yml` runs on `push` for `main` alone, so a
ref that never landed on `main` is rejected rather than published.

The gate itself lives in `scripts/wait-for-rust-run.sh`. Pull requests skip the
gate job, so its behaviour is covered by `scripts/tests/wait-for-rust-run.sh`,
which `scripts/test-workspace.sh` runs against a stubbed `gh`.

Pin the seven-character SHA tag, never the branch tag. Only the `<sha7>` tags
are immutable. Pushes to `main` also publish a floating `main` tag as a
human-facing pointer; it moves on every merge and, because publish runs are
keyed by commit rather than serialized, overlapping merges can leave it on the
older commit. The workflow intentionally does not publish `latest` at all,
because Kubernetes treats `:latest` images as `imagePullPolicy: Always`, which
can make pod restarts pull a new image without a coreconfig manifest diff.

Before relying on the Docker workflow, `divinevideo/divine-sky` must be
allow-listed in the staging and production Workload Identity providers in
`../divine-iac-coreconfig`.

Scope that allow-list to pushes on `main`, not to the repository alone. The
build job runs on pull requests too, and a pull request can edit the workflow it
runs under. Repository-only scoping therefore lets any branch that can open a
pull request mint the image-writer credential; binding the provider condition to
the `push` event on `main` is what actually keeps an unreviewed branch out of
`containers-production`.

The repository also needs these GitHub variables:

- `WORKLOAD_IDENTITY_PROVIDER_STAGING`
- `SERVICE_ACCOUNT_STAGING`
- `GCP_PROJECT_ID_STAGING`
- `WORKLOAD_IDENTITY_PROVIDER_PRODUCTION`
- `SERVICE_ACCOUNT_PRODUCTION`
- `GCP_PROJECT_ID_PRODUCTION`

## Promotion

Promote a build by pinning the target service overlay in
`../divine-iac-coreconfig` to the published seven-character SHA tag, never to
`main` or `latest`. The
`divine-sky` workflow only publishes images; it does not dispatch `image-deploy`
or update Kubernetes manifests.

Merge order for the current publish path:

1. Apply the Workload Identity allow-list changes for staging and production.
2. Merge the `divine-sky` Docker workflow.
3. Trigger the Docker workflow for `main`, or wait for the first post-merge
   `main` push, and confirm the needed SHA tags exist in both registries.
4. Pin the staging or production overlays in `../divine-iac-coreconfig` to the
   published SHA tags.
5. Register `divine-sky` with the `image-deploy` handler only after this repo
   has the dispatch app credentials needed by that automation.

## Runtime Expectations

The runtime binaries in `divine-sky` should remain compatible with Kubernetes:

- explicit env-driven bind addresses and ports
- `/health` and `/health/ready` for HTTP services
- an internal authenticated `POST /provision` surface on `divine-atbridge`
- stdout/stderr logging only
- no dependence on localhost bindings for deploy-time behavior

## Verification

Before promoting a release, validate both layers:

- `cargo check --workspace`
- `bash scripts/test-workspace.sh`
- `kustomize build` or equivalent for each new `staging` and `production` overlay in `divine-iac-coreconfig`
- confirm the referenced image tag exists in the target Artifact Registry before merging a coreconfig image bump
- verify only `divine-feedgen` and `divine-labeler` have public Gateway API resources
- verify `divine-atbridge` and `divine-handle-gateway` remain cluster-internal
