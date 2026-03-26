#!/usr/bin/env bash
# Post Vine archive videos to staging PDS as app.bsky.feed.post records.
#
# Required environment:
#   PDS_URL      — e.g. https://pds.staging.dvines.org
#   PDS_HANDLE   — account handle or DID for createSession
#   PDS_PASSWORD — account password for createSession
#   DID          — the repo DID to post into

set -euo pipefail

: "${PDS_URL:?PDS_URL must be set}"
: "${PDS_HANDLE:?PDS_HANDLE must be set}"
: "${PDS_PASSWORD:?PDS_PASSWORD must be set}"
: "${DID:?DID must be set}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

# Create session to get JWT
echo "=== Creating session ==="
session_resp=$(curl -fsSL --max-time 15 \
  -X POST "${PDS_URL}/xrpc/com.atproto.server.createSession" \
  -H "Content-Type: application/json" \
  -d "{\"identifier\":\"${PDS_HANDLE}\",\"password\":\"${PDS_PASSWORD}\"}")

JWT=$(echo "$session_resp" | jq -r '.accessJwt')
SESSION_DID=$(echo "$session_resp" | jq -r '.did')
echo "  Authenticated as ${SESSION_DID}"

upload_and_post() {
  local rkey="$1"
  local video_url="$2"
  local title="$3"
  local alt_text="$4"
  local created_at="$5"
  local aspect_w="$6"
  local aspect_h="$7"
  local post_text="$8"

  echo "=== Posting: $title ==="

  # 1. Download video
  echo "  Downloading video..."
  local video_file="${tmpdir}/${rkey}.mp4"
  curl -fsSL -o "$video_file" "$video_url"
  local video_size
  video_size=$(wc -c < "$video_file" | tr -d ' ')
  echo "  Downloaded ${video_size} bytes"

  # 2. Upload blob to PDS
  echo "  Uploading blob to PDS..."
  local upload_resp
  upload_resp=$(curl -fsSL --max-time 120 \
    -X POST "${PDS_URL}/xrpc/com.atproto.repo.uploadBlob" \
    -H "Authorization: Bearer ${JWT}" \
    -H "Content-Type: video/mp4" \
    --data-binary "@${video_file}")

  echo "  Upload response: ${upload_resp}"

  local blob_cid
  blob_cid=$(echo "$upload_resp" | jq -r '.blob.ref["$link"]')
  local blob_mime
  blob_mime=$(echo "$upload_resp" | jq -r '.blob.mimeType')
  local blob_size
  blob_size=$(echo "$upload_resp" | jq -r '.blob.size')

  echo "  Blob CID: ${blob_cid}"

  # 3. Create post record with video embed
  echo "  Creating post record (rkey: ${rkey})..."
  local record
  record=$(jq -n \
    --arg text "$post_text" \
    --arg created_at "$created_at" \
    --arg blob_cid "$blob_cid" \
    --arg blob_mime "$blob_mime" \
    --argjson blob_size "$blob_size" \
    --arg alt "$alt_text" \
    --argjson aspect_w "$aspect_w" \
    --argjson aspect_h "$aspect_h" \
    '{
      "$type": "app.bsky.feed.post",
      "text": $text,
      "createdAt": $created_at,
      "langs": ["en"],
      "embed": {
        "$type": "app.bsky.embed.video",
        "video": {
          "$type": "blob",
          "ref": { "$link": $blob_cid },
          "mimeType": $blob_mime,
          "size": $blob_size
        },
        "alt": $alt,
        "aspectRatio": {
          "width": $aspect_w,
          "height": $aspect_h
        }
      }
    }')

  local put_body
  put_body=$(jq -n \
    --arg repo "$DID" \
    --arg collection "app.bsky.feed.post" \
    --arg rkey "$rkey" \
    --argjson record "$record" \
    '{
      "repo": $repo,
      "collection": $collection,
      "rkey": $rkey,
      "record": $record
    }')

  local put_resp
  put_resp=$(curl -fsSL --max-time 30 \
    -X POST "${PDS_URL}/xrpc/com.atproto.repo.putRecord" \
    -H "Authorization: Bearer ${JWT}" \
    -H "Content-Type: application/json" \
    -d "$put_body")

  echo "  Published: ${put_resp}"
  echo ""
}

# === Video 1: "Do it for the vine" by Cameron Dallas ===
upload_and_post \
  "MA6mjTWZKEB" \
  "https://media.divine.video/9bd502ed8405d8612accbc4426be5f33ecd1564438d50226295fb0b4f7f8e9f1" \
  "Do it for the vine" \
  "Video: Do it for the vine" \
  "2014-03-04T21:02:27Z" \
  1 1 \
  "Do it for the vine

Original stats: 14,372,954 loops - 800,274 likes"

# === Video 2: "Spin around and do it for the vine" by LiveLikeDavis ===
upload_and_post \
  "hFxlUuKIIqU" \
  "https://media.divine.video/c13fc6b89327bed20f5c360d77141dc4b8b80722a4ae1f0ab1f824dfe09e25d3" \
  "Spin around and do it for the vine all night ALRIGHT" \
  "Video: Spin around and do it for the vine all night ALRIGHT 🔥🔥😎💃" \
  "2013-11-22T20:47:08Z" \
  1 1 \
  "Spin around and do it for the vine all night ALRIGHT 🔥🔥😎💃

Original stats: 326,309 loops - 111,039 likes"

echo "=== Done! ==="
echo "Verify with:"
echo "  curl -s '${PDS_URL}/xrpc/com.atproto.repo.listRecords?repo=${DID}&collection=app.bsky.feed.post' | jq"
