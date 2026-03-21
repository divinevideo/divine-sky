# PDS Operations

## Local Stack

The runtime slice uses the compose stack in [config/docker-compose.yml](/Users/rabble/code/divine/divine-sky/config/docker-compose.yml):

- `postgres` for bridge state
- `minio` plus `minio-init` for blob buckets
- `mock-blossom` for deterministic media fetches
- `mock-relay` for a real WebSocket relay endpoint during local runtime testing
- `pds` for local XRPC blob and record endpoints
- `bridge` for the AT bridge runtime container

Bring it up with:

```bash
docker compose -f config/docker-compose.yml up --build -d
```

The `pds` service builds the patched sibling checkout at `../rsky` and injects the blob-store env vars required by that fork:

- `PDS_BLOBSTORE_S3_BUCKET=pds-blobs`
- `AWS_ENDPOINT_BUCKET=pds-blobs`

## Bucket Bootstrap

[config/minio-init.sh](/Users/rabble/code/divine/divine-sky/config/minio-init.sh) creates `pds-blobs` and `bridge-blobs` by default. Override `MINIO_BUCKETS` if a test stack needs different bucket names.

## Health Checks

- `postgres`: `pg_isready`
- `minio`: `/minio/health/live`
- `mock-blossom`: `/health`
- `mock-relay`: TCP socket on `8765`
- `pds`: `/xrpc/_health`
- `bridge`: direct process execution plus connectivity checks to `pds` and `mock-relay`

## Bridge Runtime Contract

The bridge now expects these runtime values:

- `RELAY_URL`
- `RELAY_SOURCE_NAME`
- `PDS_URL`
- `PDS_AUTH_TOKEN`
- `BLOSSOM_URL`
- `DATABASE_URL`

For the local compose stack, these are supplied in [config/docker-compose.yml](/Users/rabble/code/divine/divine-sky/config/docker-compose.yml). Outside compose, export them before running `cargo run -p divine-atbridge`.

## PDS-Specific Dev Stack

Use [deploy/pds/docker-compose.yml](/Users/rabble/code/divine/divine-sky/deploy/pds/docker-compose.yml) when iterating on the PDS alone. It reuses the same MinIO bootstrap script so bucket creation stays consistent with the full local stack.
