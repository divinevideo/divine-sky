#!/usr/bin/env python3
import json
import os
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


HOST = os.environ.get("MOCK_PDS_HOST", "127.0.0.1")
PORT = int(os.environ.get("MOCK_PDS_PORT", "2583"))
FIXTURE_DIR = Path(__file__).parent / "fixtures"
SAMPLE_VIDEO = FIXTURE_DIR / "sample.mp4"
SAMPLE_DID = "did:plc:divineblackskyapplab"
SAMPLE_HANDLE = "lab.divine.video"
SAMPLE_BLOB_CID = "bafkreicwqno6pzrospmpufh6l6hs7y26v4jdd4zxq5x6j6wxmvtow2g4zu"

PROFILE_RECORD = {
    "uri": f"at://{SAMPLE_DID}/app.bsky.actor.profile/self",
    "cid": "bafyreie5ew6wq7n6xv7qqw3v7x6r6kllmockprofilecid000000000000",
    "value": {
        "$type": "app.bsky.actor.profile",
        "displayName": "Divine AppView Lab",
        "description": "Fixture-backed repo for the Blacksky appview viewer lab.",
        "website": "https://divine.video",
        "createdAt": "2026-03-21T00:00:00Z",
    },
}

POST_RECORDS = [
    {
        "uri": f"at://{SAMPLE_DID}/app.bsky.feed.post/3labpost1",
        "cid": "bafyreif7j7mgy6wq3qg2r4labpostcid0000000000000000000000001",
        "value": {
            "$type": "app.bsky.feed.post",
            "text": "Divine Blacksky lab sample clip",
            "createdAt": "2026-03-21T00:05:00Z",
            "embed": {
                "$type": "app.bsky.embed.video",
                "video": {
                    "$type": "blob",
                    "ref": {"$link": SAMPLE_BLOB_CID},
                    "mimeType": "video/mp4",
                    "size": SAMPLE_VIDEO.stat().st_size if SAMPLE_VIDEO.exists() else 0,
                },
                "alt": "A generated one-second sample clip for the local appview lab.",
                "aspectRatio": {"width": 320, "height": 180},
            },
        },
    },
    {
        "uri": f"at://{SAMPLE_DID}/app.bsky.feed.post/3labpost2",
        "cid": "bafyreie4zzw6k7j4mabpostcid000000000000000000000000000002",
        "value": {
            "$type": "app.bsky.feed.post",
            "text": "Trending fixture post for author, detail, and search views",
            "createdAt": "2026-03-21T00:10:00Z",
            "embed": {
                "$type": "app.bsky.embed.video",
                "video": {
                    "$type": "blob",
                    "ref": {"$link": SAMPLE_BLOB_CID},
                    "mimeType": "video/mp4",
                    "size": SAMPLE_VIDEO.stat().st_size if SAMPLE_VIDEO.exists() else 0,
                },
                "alt": "The same sample clip reused to exercise multiple feed surfaces.",
                "aspectRatio": {"width": 320, "height": 180},
            },
        },
    },
]


class MockPdsHandler(BaseHTTPRequestHandler):
    server_version = "mock-pds/0.1"

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        query = urllib.parse.parse_qs(parsed.query)

        if parsed.path == "/":
            body = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Divine Mock PDS</title>
</head>
<body style="font-family: sans-serif; max-width: 860px; margin: 0 auto; padding: 2rem; line-height: 1.6;">
  <p style="color:#666">Blacksky AppView Lab</p>
  <h1>Divine Mock PDS</h1>
  <p>Fixture-backed ATProto surface for the local appview lab. It exposes one sample repo and one sample video blob so the indexer, media view, and viewer can run without a full PDS stack.</p>
  <h2>Fixture</h2>
  <p>Handle: <code>{SAMPLE_HANDLE}</code><br>DID: <code>{SAMPLE_DID}</code><br>Blob CID: <code>{SAMPLE_BLOB_CID}</code></p>
  <h2>Endpoints</h2>
  <ul>
    <li><a href="/xrpc/_health"><code>/xrpc/_health</code></a></li>
    <li><a href="/xrpc/com.atproto.sync.listRepos"><code>/xrpc/com.atproto.sync.listRepos</code></a></li>
    <li><a href="/xrpc/com.atproto.repo.listRecords?repo={urllib.parse.quote(SAMPLE_DID)}&amp;collection=app.bsky.actor.profile"><code>/xrpc/com.atproto.repo.listRecords?repo={SAMPLE_DID}&amp;collection=app.bsky.actor.profile</code></a></li>
    <li><a href="/xrpc/com.atproto.repo.listRecords?repo={urllib.parse.quote(SAMPLE_DID)}&amp;collection=app.bsky.feed.post"><code>/xrpc/com.atproto.repo.listRecords?repo={SAMPLE_DID}&amp;collection=app.bsky.feed.post</code></a></li>
    <li><a href="/xrpc/com.atproto.sync.getBlob?did={urllib.parse.quote(SAMPLE_DID)}&amp;cid={SAMPLE_BLOB_CID}"><code>/xrpc/com.atproto.sync.getBlob?did={SAMPLE_DID}&amp;cid={SAMPLE_BLOB_CID}</code></a></li>
  </ul>
</body>
</html>""".encode("utf-8")
            self._write_bytes(200, body, "text/html; charset=utf-8")
            return

        if parsed.path == "/xrpc/_health":
            self._write_bytes(200, b"ok", "text/plain; charset=utf-8")
            return

        if parsed.path == "/xrpc/com.atproto.sync.listRepos":
            self._write_json(
                200,
                {
                    "repos": [
                        {
                            "did": SAMPLE_DID,
                            "handle": SAMPLE_HANDLE,
                            "head": "mock-head",
                            "rev": "mock-rev",
                            "active": True,
                        }
                    ]
                },
            )
            return

        if parsed.path == "/xrpc/com.atproto.repo.listRecords":
            repo = query.get("repo", [""])[0]
            collection = query.get("collection", [""])[0]
            if repo != SAMPLE_DID:
                self._write_json(200, {"records": []})
                return

            if collection == "app.bsky.actor.profile":
                self._write_json(200, {"records": [PROFILE_RECORD]})
                return

            if collection == "app.bsky.feed.post":
                self._write_json(200, {"records": POST_RECORDS})
                return

            self._write_json(200, {"records": []})
            return

        if parsed.path == "/xrpc/com.atproto.sync.getBlob":
            did = query.get("did", [""])[0]
            cid = query.get("cid", [""])[0]
            if did != SAMPLE_DID or cid != SAMPLE_BLOB_CID or not SAMPLE_VIDEO.exists():
                self._write_json(404, {"error": "not_found"})
                return

            body = SAMPLE_VIDEO.read_bytes()
            self._write_bytes(200, body, "video/mp4")
            return

        self._write_json(404, {"error": "not_found"})

    def log_message(self, format, *args):
        return

    def _write_json(self, status: int, payload: dict):
        body = json.dumps(payload).encode("utf-8")
        self._write_bytes(status, body, "application/json")

    def _write_bytes(self, status: int, body: bytes, content_type: str):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    server = ThreadingHTTPServer((HOST, PORT), MockPdsHandler)
    print(f"mock pds listening on http://{HOST}:{PORT}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
