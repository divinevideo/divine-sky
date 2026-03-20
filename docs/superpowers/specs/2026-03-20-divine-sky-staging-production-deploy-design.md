# divine-sky Staging and Production Deploy Design

**Date:** 2026-03-20
**Status:** Approved

## Purpose

Deploy the runnable `divine-sky` services into the existing Divine staging and production platform managed by `divine-iac-coreconfig`, while hardening the runtimes in this repository so they match cluster expectations.

This design covers:

- `divine-atbridge`
- `divine-handle-gateway`
- `divine-feedgen`
- `divine-labeler`

The source of truth for staging and production deployment manifests is `../divine-iac-coreconfig`. This repository owns the binary/runtime contract those manifests depend on.

## Deployment Model

`divine-iac-coreconfig` remains the infrastructure and GitOps control plane:

- Terragrunt/OpenTofu manages cluster and cloud resources.
- ArgoCD manages Kubernetes applications.
- `divine-sky` services are represented as one ArgoCD application per runtime.
- Only `staging` and `production` are in scope for this first pass.

## Namespace and Service Layout

Create a dedicated `sky` namespace in `divine-iac-coreconfig` for the `divine-sky` services.

Services in that namespace:

- **`divine-atbridge`**
  - Internal worker deployment
  - No public route
  - Singleton rollout initially
- **`divine-handle-gateway`**
  - Internal HTTP service
  - No public route
  - Cluster DNS access only
- **`divine-feedgen`**
  - Public HTTP/XRPC service
  - Exposed through Gateway API + external DNS
- **`divine-labeler`**
  - Public ATProto label-query service
  - Internal webhook endpoint on the same service
  - Exposed through Gateway API + external DNS

## Runtime Hardening in divine-sky

### Shared expectations

Every deployable runtime must:

- bind on an explicit env-driven host/port instead of hardcoded localhost
- expose health endpoints suitable for Kubernetes
- log to stdout/stderr only
- document required environment variables
- avoid assuming a local compose-only environment

### divine-handle-gateway

Required changes:

- add env-driven bind address and port
- add `GET /health`
- add `GET /health/ready`
- keep lifecycle endpoints bearer-protected
- remain internal-only

### divine-feedgen

Required changes:

- add env-driven bind address and port
- add `GET /health`
- add `GET /health/ready`
- keep existing XRPC endpoints unchanged

Intended public hostnames:

- staging: `feed.staging.dvines.org`
- production: `feed.divine.video`

### divine-labeler

Required changes:

- keep env-driven port
- keep `GET /health`
- add `GET /health/ready`
- keep webhook bearer auth
- keep ATProto `queryLabels` endpoint unchanged

Intended public hostnames:

- staging: `labeler.staging.dvines.org`
- production: `labeler.divine.video`

### divine-atbridge

Required changes:

- keep worker runtime behavior
- add a lightweight health surface for liveness/readiness
- add env-driven health bind address/port if the health surface is HTTP
- keep singleton replica count in staging and production for now

This first pass does not add multi-replica relay-consumer coordination. Instead, deployment policy keeps `replicas: 1` until duplicate-consumer behavior is explicitly designed.

## Configuration and Secrets

Runtime code in `divine-sky` defines the contract. Actual staging/production values and secret wiring live in `divine-iac-coreconfig`.

### divine-atbridge

Expected env/secrets:

- `RELAY_URL`
- `PDS_URL`
- `PDS_AUTH_TOKEN`
- `BLOSSOM_URL`
- `DATABASE_URL`
- `S3_ENDPOINT`
- `S3_BUCKET`
- `RELAY_SOURCE_NAME`
- health bind env if added

### divine-handle-gateway

Expected env/secrets:

- `DATABASE_URL`
- `KEYCAST_ATPROTO_TOKEN`
- `ATPROTO_PROVISIONING_URL`
- `ATPROTO_PROVISIONING_TOKEN`
- `ATPROTO_KEYCAST_SYNC_URL`
- `ATPROTO_NAME_SERVER_SYNC_URL`
- `ATPROTO_NAME_SERVER_SYNC_TOKEN`
- bind env

### divine-feedgen

Expected env/secrets:

- bind env
- any feed identity/base URL env introduced during hardening

### divine-labeler

Expected env/secrets:

- `LABELER_DID`
- `LABELER_SIGNING_KEY`
- `DATABASE_URL`
- `WEBHOOK_TOKEN`
- `PORT` or shared bind env

Secrets in `divine-iac-coreconfig` should be managed with `ExternalSecret` and environment-specific GCP Secret Manager keys, following the existing `keycast` and `divine-push-service` patterns.

## Coreconfig Structure

For each service, add:

- `k8s/applications/<service>/base/`
- `k8s/applications/<service>/overlays/staging/`
- `k8s/applications/<service>/overlays/production/`
- `k8s/argocd/apps/<service>.yaml`

Namespace:

- add `k8s/cluster-config/namespaces/sky.yaml`

Exposure:

- `divine-feedgen` and `divine-labeler` get `HTTPRoute`
- `divine-atbridge` and `divine-handle-gateway` do not

## Rollout Policy

### Staging

- auto-deploy through the existing coreconfig workflow
- `divine-atbridge`: 1 replica
- `divine-handle-gateway`: 2 replicas once health/bind work is complete
- `divine-feedgen`: 2 replicas
- `divine-labeler`: 2 replicas

### Production

- manual approval only, matching existing coreconfig policy
- `divine-atbridge`: 1 replica initially
- `divine-handle-gateway`: 2 replicas
- `divine-feedgen`: 2 replicas
- `divine-labeler`: 2 replicas

## Verification

### divine-sky

- focused Rust tests for each hardened service
- `cargo check` for touched crates
- route tests for health endpoints and bind/env config where practical

### divine-iac-coreconfig

- `kustomize build` or equivalent manifest rendering for each new staging/production overlay
- verify ArgoCD app definitions point to the correct overlay paths
- verify external routes only exist for `feedgen` and `labeler`
- verify internal-only services have no public Gateway API resources

## Deferred

- POC/test overlays
- multi-replica `divine-atbridge` coordination
- metrics/ServiceMonitor wiring beyond basic health
- autoscaling policy beyond fixed initial replica counts
