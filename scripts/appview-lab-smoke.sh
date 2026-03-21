#!/usr/bin/env bash
set -euo pipefail

check() {
  local url="$1"
  local expected="$2"
  local status
  status="$(curl -s -o /dev/null -w "%{http_code}" "$url")"
  if [[ "$status" != "$expected" ]]; then
    echo "expected $expected from $url, got $status" >&2
    exit 1
  fi
  echo "$url -> $status"
}

check_cors() {
  local url="$1"
  local origin="$2"
  local header
  header="$(curl -sS -D - -o /dev/null -H "Origin: ${origin}" "$url" | tr -d '\r' | awk -F': ' 'tolower($1) == "access-control-allow-origin" { print $2 }')"
  if [[ "$header" != "$origin" ]]; then
    echo "expected access-control-allow-origin ${origin} from $url, got ${header:-<missing>}" >&2
    exit 1
  fi
  echo "$url -> access-control-allow-origin ${header}"
}

check_contains() {
  local url="$1"
  local expected="$2"
  local body
  body="$(curl -fsS "$url")"
  if [[ "$body" != *"$expected"* ]]; then
    echo "expected $url to contain: $expected" >&2
    exit 1
  fi
  echo "$url -> contains $expected"
}

check_contains "http://127.0.0.1:3004/" "Divine AppView Lab"
check "http://127.0.0.1:3004/health" "200"
check "http://127.0.0.1:3004/health/ready" "200"
check_contains "http://127.0.0.1:3002/" "Divine Blacksky Feed Generator"
check "http://127.0.0.1:3002/health" "200"
check "http://127.0.0.1:3002/xrpc/app.bsky.feed.describeFeedGenerator" "200"
check_contains "http://127.0.0.1:2583/" "Divine Mock PDS"
check "http://127.0.0.1:2583/xrpc/_health" "200"
check_contains "http://127.0.0.1:3100/" "Divine Media View"
check "http://127.0.0.1:3100/health" "200"
check "http://127.0.0.1:4173" "200"
check "http://127.0.0.1:3100/streams/did/plc/divineblackskyapplab/bafkreicwqno6pzrospmpufh6l6hs7y26v4jdd4zxq5x6j6wxmvtow2g4zu.mp4" "200"
check_cors "http://127.0.0.1:3002/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:plc:divine.feed/app.bsky.feed.generator/latest&limit=12" "http://127.0.0.1:4173"
check_cors "http://127.0.0.1:3100/playlists/did/plc/divineblackskyapplab/bafkreicwqno6pzrospmpufh6l6hs7y26v4jdd4zxq5x6j6wxmvtow2g4zu.m3u8" "http://127.0.0.1:4173"
