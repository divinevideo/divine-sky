#!/usr/bin/env python3
import hashlib
import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


SEED_DIR = Path(os.environ.get("BLOSSOM_SEED_DIR", Path(__file__).parent / "seed"))


def guess_content_type(path: Path) -> str:
    suffix = path.suffix.lower()
    if suffix == ".mp4":
        return "video/mp4"
    if suffix == ".png":
        return "image/png"
    if suffix in {".jpg", ".jpeg"}:
        return "image/jpeg"
    return "application/octet-stream"


class MockBlossomHandler(BaseHTTPRequestHandler):
    server_version = "mock-blossom/0.1"

    def do_GET(self):
        if self.path == "/health":
            self._write_bytes(200, b"ok", "text/plain; charset=utf-8")
            return

        seed_root = SEED_DIR.resolve()
        requested = (seed_root / self.path.lstrip("/")).resolve()
        if seed_root not in requested.parents and requested != seed_root:
            self._write_json(400, {"error": "invalid_path"})
            return

        if not requested.exists() or not requested.is_file():
            self._write_json(404, {"error": "not_found"})
            return

        body = requested.read_bytes()
        sha256_hex = hashlib.sha256(body).hexdigest()
        self.send_response(200)
        self.send_header("Content-Type", guess_content_type(requested))
        self.send_header("Content-Length", str(len(body)))
        self.send_header("ETag", sha256_hex)
        self.end_headers()
        self.wfile.write(body)

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
    host = os.environ.get("BLOSSOM_HOST", "0.0.0.0")
    port = int(os.environ.get("BLOSSOM_PORT", "8080"))
    server = ThreadingHTTPServer((host, port), MockBlossomHandler)
    print(f"mock blossom listening on http://{host}:{port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
