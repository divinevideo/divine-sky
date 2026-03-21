# Divine Blacksky AppView Lab

This lab stack is the local Divine-only ATProto read path described in the appview plan.

It brings up:

- PostgreSQL for the appview read model
- a fixture-backed mock PDS on port `2583` when `DIVINE_PDS_URL` is left at the default local endpoint
- an optional external relay container when `APPVIEW_ENABLE_RELAY=true`
- a tiny media-view proxy on port `3100`
- `divine-appview-indexer` as a oneshot backfill
- `divine-appview` on port `3004`
- `divine-feedgen` on port `3002`
- `apps/divine-blacksky-viewer` on port `4173`

## Usage

1. Copy `deploy/appview-lab/env.example` to `deploy/appview-lab/.env`.
2. Set `DIVINE_PDS_URL` to the Divine PDS you want to index.
3. Run `bash scripts/appview-lab-up.sh`.
4. Run `bash scripts/appview-lab-smoke.sh`.
5. Open `http://127.0.0.1:4173`.

## Notes

- The indexer currently runs as a oneshot backfill on startup. Relay-driven live refresh can be layered in behind the same store and route contracts without changing the appview or viewer contracts.
- Local bootstrap defaults to `http://127.0.0.1:2583` and serves two sample posts from the mock PDS so the viewer is not empty on first run.
- Point `DIVINE_PDS_URL` at a real Divine PDS if you want the same appview/feedgen/viewer stack to index live repo data instead of the fixture repo.
- The relay container is optional for the first lab because the current indexer defaults to `DIVINE_INDEXER_ONESHOT=true`.
- If you want to exercise a live relay later, set `APPVIEW_ENABLE_RELAY=true` and override `RSKY_RELAY_IMAGE` if your fork publishes under a different registry path.
- Media playback uses the local media-view proxy to serve deterministic playlist and blob URLs for the lab.
