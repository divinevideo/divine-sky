#!/usr/bin/env python3
import os
import urllib.error
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


PDS_BASE_URL = os.environ.get("DIVINE_PDS_URL", "http://127.0.0.1:3000").rstrip("/")
HOST = os.environ.get("APPVIEW_MEDIA_HOST", "127.0.0.1")
PORT = int(os.environ.get("APPVIEW_MEDIA_PORT", "3100"))
VIEWER_ORIGIN = os.environ.get("VIEWER_ORIGIN", "").strip()
SAMPLE_DID = os.environ.get("APPVIEW_MEDIA_SAMPLE_DID", "did:plc:divineblackskyapplab")
SAMPLE_BLOB_CID = os.environ.get(
    "APPVIEW_MEDIA_SAMPLE_CID",
    "bafkreicwqno6pzrospmpufh6l6hs7y26v4jdd4zxq5x6j6wxmvtow2g4zu",
)


def path_to_did(parts):
    return ":".join(parts)


def did_to_path(did):
    return did.replace(":", "/")


class MediaViewHandler(BaseHTTPRequestHandler):
    def do_OPTIONS(self):
        self.send_response(204)
        self.end_headers()

    def end_headers(self):
        request_origin = self.headers.get("Origin", "").strip()
        allow_origin = VIEWER_ORIGIN or request_origin or "*"
        self.send_header("Vary", "Origin")
        self.send_header("Access-Control-Allow-Origin", allow_origin)
        self.send_header("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Range, Content-Type")
        self.send_header(
            "Access-Control-Expose-Headers",
            "Content-Length, Content-Range, Accept-Ranges",
        )
        super().end_headers()

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path == "/":
            sample_did_path = did_to_path(SAMPLE_DID)
            body = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Divine Media View</title>
</head>
<body style="font-family: sans-serif; max-width: 860px; margin: 0 auto; padding: 2rem; line-height: 1.6;">
  <p style="color:#666">Blacksky AppView Lab</p>
  <h1>Divine Media View</h1>
  <p>Browser-facing media proxy for the local viewer lab. It exposes HLS-style playlist URLs, direct MP4 stream URLs, and thumbnails, and fetches blob bytes from <code>{PDS_BASE_URL}</code>.</p>
  <h2>Endpoints</h2>
  <ul>
    <li><a href="/health"><code>/health</code></a></li>
    <li><a href="/playlists/{sample_did_path}/{SAMPLE_BLOB_CID}.m3u8"><code>/playlists/&lt;did path&gt;/&lt;cid&gt;.m3u8</code></a></li>
    <li><a href="/streams/{sample_did_path}/{SAMPLE_BLOB_CID}.mp4"><code>/streams/&lt;did path&gt;/&lt;cid&gt;.mp4</code></a></li>
    <li><a href="/thumbnails/{sample_did_path}/{SAMPLE_BLOB_CID}.jpg"><code>/thumbnails/&lt;did path&gt;/&lt;cid&gt;.jpg</code></a></li>
  </ul>
  <h2>Fixture</h2>
  <p><code>{SAMPLE_DID}</code><br><code>{SAMPLE_BLOB_CID}</code></p>
</body>
</html>""".encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        if parsed.path == "/health":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok")
            return

        segments = [segment for segment in parsed.path.split("/") if segment]
        if len(segments) >= 4 and segments[0] == "playlists":
            did = path_to_did(segments[1:-1])
            cid = segments[-1].replace(".m3u8", "")
            playlist = (
                "#EXTM3U\n"
                "#EXT-X-VERSION:7\n"
                "#EXT-X-TARGETDURATION:1\n"
                "#EXT-X-MEDIA-SEQUENCE:0\n"
                f"#EXTINF:1.0,\n/streams/{did.replace(':', '/')}/{cid}.mp4\n"
                "#EXT-X-ENDLIST\n"
            )
            self.send_response(200)
            self.send_header("Content-Type", "application/vnd.apple.mpegurl")
            self.end_headers()
            self.wfile.write(playlist.encode("utf-8"))
            return

        if len(segments) >= 4 and segments[0] in ("streams", "blobs"):
            did = path_to_did(segments[1:-1])
            cid = segments[-1].replace(".mp4", "")
            query = urllib.parse.urlencode({"did": did, "cid": cid})
            upstream = f"{PDS_BASE_URL}/xrpc/com.atproto.sync.getBlob?{query}"
            try:
                with urllib.request.urlopen(upstream) as response:
                    body = response.read()
                    content_type = response.headers.get("Content-Type", "application/octet-stream")
                    self.send_response(200)
                    self.send_header("Content-Type", content_type)
                    self.send_header("Content-Length", str(len(body)))
                    self.send_header("Accept-Ranges", "bytes")
                    self.end_headers()
                    self.wfile.write(body)
            except urllib.error.HTTPError as error:
                status = error.code or 502
                body = f'{{"error":"upstream_blob_fetch_failed","status":{status}}}'.encode("utf-8")
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            return

        if len(segments) >= 4 and segments[0] == "thumbnails":
            cid = segments[-1].replace(".jpg", "")
            svg = f"""
<svg xmlns="http://www.w3.org/2000/svg" width="960" height="540">
  <rect width="100%" height="100%" fill="#10131a" />
  <rect x="80" y="80" width="800" height="380" rx="32" fill="#1d2433" />
  <text x="120" y="270" font-size="42" fill="#ffa726" font-family="monospace">{cid}</text>
</svg>
""".strip()
            self.send_response(200)
            self.send_header("Content-Type", "image/svg+xml")
            self.end_headers()
            self.wfile.write(svg.encode("utf-8"))
            return

        self.send_response(404)
        self.end_headers()


def main():
    server = ThreadingHTTPServer((HOST, PORT), MediaViewHandler)
    print(f"media-view server listening on http://{HOST}:{PORT}")
    server.serve_forever()


if __name__ == "__main__":
    main()
