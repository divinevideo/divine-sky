#!/usr/bin/env bash
# Bridge a divine.video user's NIP-71 videos to Bluesky via PDS + video.bsky.app
#
# Required environment:
#   PDS_URL          — e.g. https://pds.staging.dvines.org
#   PDS_HANDLE       — account handle on PDS
#   PDS_PASSWORD     — account password
#   PDS_ADMIN_PW     — PDS admin password (for service auth)
#   NOSTR_PUBKEY_HEX — hex pubkey of the Nostr user
#   RELAY_URL        — Nostr relay WebSocket URL (e.g. wss://relay.divine.video)
#
# Optional:
#   MAX_VIDEOS       — max videos to bridge (default: all)
#   DRY_RUN          — set to "true" to skip uploads

set -euo pipefail

: "${PDS_URL:?}"
: "${PDS_HANDLE:?}"
: "${PDS_PASSWORD:?}"
: "${PDS_ADMIN_PW:?}"
: "${NOSTR_PUBKEY_HEX:?}"
: "${RELAY_URL:?}"

MAX_VIDEOS="${MAX_VIDEOS:-999}"
DRY_RUN="${DRY_RUN:-false}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "=== Bridge: ${NOSTR_PUBKEY_HEX:0:16}... to ${PDS_HANDLE} ==="

# 1. Create PDS session
SESSION=$(curl -fsSL -X POST "${PDS_URL}/xrpc/com.atproto.server.createSession" \
  -H "Content-Type: application/json" \
  -d "{\"identifier\":\"${PDS_HANDLE}\",\"password\":\"${PDS_PASSWORD}\"}")
JWT=$(echo "$SESSION" | jq -r '.accessJwt')
DID=$(echo "$SESSION" | jq -r '.did')
echo "Authenticated as ${DID}"

# 2. Fetch video events from relay
echo "Fetching video events from relay..."
REQ_MSG=$(printf '["REQ","bridge",{"authors":["%s"],"kinds":[34235,34236],"limit":%d}]' "$NOSTR_PUBKEY_HEX" "$MAX_VIDEOS")
echo "  REQ: ${REQ_MSG}"
set +e
wscat -c "${RELAY_URL}" -x "$REQ_MSG" -w 8 > "${tmpdir}/events.jsonl" 2>"${tmpdir}/wscat.err"
set -e
echo "  wscat stderr: $(cat ${tmpdir}/wscat.err | head -1)"

VIDEO_COUNT=$(grep -c '^\["EVENT"' "${tmpdir}/events.jsonl" || true)
echo "Found ${VIDEO_COUNT} video events"

if [ "$VIDEO_COUNT" -eq 0 ]; then
  echo "No videos found. Exiting."
  exit 0
fi

# 3. Process each video
BRIDGED=0
FAILED=0
SKIPPED=0

grep '^\["EVENT"' "${tmpdir}/events.jsonl" | while IFS= read -r line; do
  TITLE=$(echo "$line" | jq -r '.[2].tags[] | select(.[0]=="title") | .[1]' 2>/dev/null | head -1)
  DTAG=$(echo "$line" | jq -r '.[2].tags[] | select(.[0]=="d") | .[1]' 2>/dev/null | head -1)
  VIDEO_URL=$(echo "$line" | jq -r '[.[2].tags[] | select(.[0]=="imeta") | .[1:][]] | map(select(startswith("url "))) | .[0]' 2>/dev/null | sed 's/^url //')
  DIM=$(echo "$line" | jq -r '[.[2].tags[] | select(.[0]=="imeta") | .[1:][]] | map(select(startswith("dim "))) | .[0]' 2>/dev/null | sed 's/^dim //')
  SUMMARY=$(echo "$line" | jq -r '.[2].tags[] | select(.[0]=="summary") | .[1]' 2>/dev/null | head -1)

  # Parse dimensions
  W=$(echo "$DIM" | cut -dx -f1)
  H=$(echo "$DIM" | cut -dx -f2)
  # GCD reduce for aspect ratio
  if [ -n "$W" ] && [ -n "$H" ]; then
    A=$W; B=$H
    while [ "$B" -ne 0 ] 2>/dev/null; do T=$B; B=$((A % B)); A=$T; done
    AW=$((W / A)); AH=$((H / A))
  else
    AW=1; AH=1
  fi

  if [ -z "$VIDEO_URL" ] || [ "$VIDEO_URL" = "null" ]; then
    echo "  SKIP: ${TITLE:-unnamed} (no video URL)"
    continue
  fi

  echo "--- ${TITLE:-unnamed} (${DIM:-?}) ---"

  if [ "$DRY_RUN" = "true" ]; then
    echo "  DRY RUN: would bridge ${VIDEO_URL}"
    continue
  fi

  # Download video
  VIDEO_FILE="${tmpdir}/video_${DTAG:0:12}.mp4"
  if ! curl -fsSL -o "$VIDEO_FILE" --max-time 120 "$VIDEO_URL" 2>/dev/null; then
    echo "  FAIL: download failed"
    continue
  fi
  FILE_SIZE=$(wc -c < "$VIDEO_FILE" | tr -d ' ')
  echo "  Downloaded: ${FILE_SIZE} bytes"

  # Check size limit (50MB for video.bsky.app)
  if [ "$FILE_SIZE" -gt 52428800 ]; then
    echo "  SKIP: too large (${FILE_SIZE} > 50MB)"
    continue
  fi

  # Get service auth for video upload
  SVC_AUTH=$(curl -fsSL "${PDS_URL}/xrpc/com.atproto.server.getServiceAuth?aud=did:web:$(echo ${PDS_URL} | sed 's|https://||')&lxm=com.atproto.repo.uploadBlob" \
    -H "Authorization: Bearer ${JWT}" | jq -r '.token')

  # Upload to video.bsky.app
  UPLOAD=$(curl -fsSL -X POST "https://video.bsky.app/xrpc/app.bsky.video.uploadVideo?did=${DID}&name=${DTAG:0:12}.mp4" \
    -H "Authorization: Bearer ${SVC_AUTH}" \
    -H "Content-Type: video/mp4" \
    --data-binary "@${VIDEO_FILE}" 2>/dev/null || echo '{"state":"UPLOAD_FAILED"}')

  JOB_ID=$(echo "$UPLOAD" | jq -r '.jobId // empty')
  if [ -z "$JOB_ID" ]; then
    echo "  FAIL: upload failed - $(echo "$UPLOAD" | jq -r '.error // .state')"
    continue
  fi
  echo "  Uploaded: job ${JOB_ID}"

  # Poll for completion (max 2 minutes)
  BLOB_CID=""
  BLOB_SIZE=""
  for i in $(seq 1 12); do
    sleep 10
    STAT=$(curl -fsSL "https://video.bsky.app/xrpc/app.bsky.video.getJobStatus?jobId=${JOB_ID}" 2>/dev/null || echo '{}')
    STATE=$(echo "$STAT" | jq -r '.jobStatus.state // "unknown"')

    if [ "$STATE" = "JOB_STATE_COMPLETED" ]; then
      BLOB_CID=$(echo "$STAT" | jq -r '.jobStatus.blob.ref."$link"')
      BLOB_SIZE=$(echo "$STAT" | jq -r '.jobStatus.blob.size')
      echo "  Transcoded: ${BLOB_CID} (${BLOB_SIZE} bytes)"
      break
    elif [ "$STATE" = "JOB_STATE_FAILED" ]; then
      echo "  FAIL: transcoding failed"
      break
    fi
  done

  if [ -z "$BLOB_CID" ]; then
    echo "  FAIL: no blob after polling"
    continue
  fi

  # Build post text
  POST_TEXT="${TITLE:-Video}"
  if [ -n "$SUMMARY" ] && [ "$SUMMARY" != "null" ]; then
    POST_TEXT="${POST_TEXT}\n\n${SUMMARY}"
  fi

  # Create post record
  CREATED_AT=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)
  RECORD=$(curl -fsSL -X POST "${PDS_URL}/xrpc/com.atproto.repo.createRecord" \
    -H "Authorization: Bearer ${JWT}" \
    -H "Content-Type: application/json" \
    -d "{
      \"repo\": \"${DID}\",
      \"collection\": \"app.bsky.feed.post\",
      \"record\": {
        \"\$type\": \"app.bsky.feed.post\",
        \"text\": $(echo -e "$POST_TEXT" | jq -Rs .),
        \"createdAt\": \"${CREATED_AT}\",
        \"langs\": [\"en\"],
        \"embed\": {
          \"\$type\": \"app.bsky.embed.video\",
          \"video\": {
            \"\$type\": \"blob\",
            \"ref\": { \"\$link\": \"${BLOB_CID}\" },
            \"mimeType\": \"video/mp4\",
            \"size\": ${BLOB_SIZE}
          },
          \"aspectRatio\": { \"width\": ${AW}, \"height\": ${AH} }
        }
      }
    }" 2>/dev/null || echo '{"error":"createRecord failed"}')

  RKEY=$(echo "$RECORD" | jq -r '.uri // empty' | awk -F/ '{print $NF}')
  if [ -n "$RKEY" ]; then
    echo "  Posted: https://bsky.app/profile/${DID}/post/${RKEY}"
  else
    echo "  FAIL: $(echo "$RECORD" | jq -r '.error // "unknown"')"
  fi

  # Clean up video file
  rm -f "$VIDEO_FILE"

  # Rate limit: wait between videos
  sleep 2
done

echo ""
echo "=== Bridge complete ==="
