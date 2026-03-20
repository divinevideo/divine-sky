# Mock Blossom

`server.py` serves static files from `seed/` and exposes `/health` for compose health checks.

Add fixture files under `seed/` when exercising the local stack outside the Rust test suite.
