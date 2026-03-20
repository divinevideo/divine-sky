# Media Path

## Chosen Path

The active runtime path is `divine-atbridge` inline media handling. `divine-video-worker` remains a helper crate for shared blob utilities, but it is not a second publish path.

## Publish Flow

1. Read the NIP-71 event in `divine-atbridge`.
2. Extract the Blossom URL and required source hash from the `x` tag.
3. Fetch bytes from Blossom with bounded HTTP timeouts.
4. Verify the fetched bytes against the expected SHA-256 before upload.
5. Upload the verified bytes to the PDS with `com.atproto.repo.uploadBlob`.
6. Publish the AT record with `com.atproto.repo.putRecord`.
7. Persist lineage:
   - `asset_manifest.source_sha256`
   - `asset_manifest.at_blob_cid`
   - `record_mappings.cid`
   - `record_mappings.status`

## Profile Assets

Kind `0` profile sync is one-way from Nostr to ATProto. Avatar and banner assets follow the same fetch-then-upload contract, but they do not require a source hash tag.

## Local Testing

Use the local stack in [config/docker-compose.yml](/Users/rabble/code/divine/divine-sky/config/docker-compose.yml). The mock Blossom service is served from `config/mock-blossom/server.py`, and MinIO buckets are bootstrapped by [config/minio-init.sh](/Users/rabble/code/divine/divine-sky/config/minio-init.sh).
