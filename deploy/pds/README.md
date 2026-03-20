# rsky-pds Local Development

Run a self-operated ATProto PDS (Personal Data Server) backed by [rsky](https://github.com/blacksky-algorithms/rsky) for local development and testing.

## Prerequisites

- Docker and Docker Compose v2+
- `curl` (for health checks)

## Quick Start

```bash
# 1. Create your .env from the template
cp env.example .env
# Edit .env to set PDS_ADMIN_PASSWORD and any other values.

# 2. Start all services
docker compose up -d

# 3. Verify
curl http://localhost:2583/xrpc/_health
# Expected: {"version":"..."}
```

Services started:

| Service     | Port(s)           | Purpose                          |
|-------------|-------------------|----------------------------------|
| pds         | 2583              | ATProto PDS (XRPC)              |
| postgres    | 5433 (host)       | PDS state database (PostgreSQL 16) |
| minio       | 9000 (API), 9001 (console) | S3-compatible blob storage |

## Health Check

```bash
curl -f http://localhost:2583/xrpc/_health
```

Returns HTTP 200 with a JSON body when the PDS is ready.

## Handle Resolution

For production, handles under `*.divine.video` must resolve to this PDS. Two methods:

### DNS (recommended for production)

Add a wildcard DNS record:

```
*.divine.video.  IN  A  <server-ip>
```

And a `_atproto` TXT record for each handle:

```
_atproto.alice.divine.video.  IN  TXT  "did=did:plc:abc123..."
```

### HTTP Well-Known (alternative)

Serve `/.well-known/atproto-did` at the handle hostname, returning the DID as plain text.

For local development, add entries to `/etc/hosts`:

```
127.0.0.1  pds.divine.video
127.0.0.1  alice.divine.video
```

## Requesting BGS Crawl for Federation

After the PDS is running and has accounts, request the BGS (Big Graph Service) to crawl it so records appear in the Bluesky network:

```bash
curl -X POST "https://bgs.bsky.network/xrpc/com.atproto.sync.requestCrawl" \
  -H "Content-Type: application/json" \
  -d '{"hostname": "pds.divine.video"}'
```

This is a one-time operation per PDS hostname. The BGS will begin subscribing to the PDS firehose.

## Stopping

```bash
docker compose down          # stop services, keep data
docker compose down -v       # stop services and delete volumes
```

## Troubleshooting

**PDS fails to start:** Check logs with `docker compose logs pds`. Common issues:
- Database not ready (the health check dependency should handle this)
- Invalid `PDS_HOSTNAME` or `PDS_SERVICE_DID`

**Cannot connect to MinIO:** The PDS connects to MinIO via the Docker network (`http://minio:9000`). From the host, use `http://localhost:9000`. Access the MinIO console at `http://localhost:9001`.

**Handle resolution fails locally:** Ensure `/etc/hosts` entries are set and the PDS is reachable at port 2583.
