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
- verify only `divine-feedgen` and `divine-labeler` have public Gateway API resources
- verify `divine-atbridge` and `divine-handle-gateway` remain cluster-internal
