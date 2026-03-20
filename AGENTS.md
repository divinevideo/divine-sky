# Repository Guidelines

## Project Structure & Module Organization

The Rust workspace lives at the repository root in `crates/`. Current crates are `divine-atbridge`, `divine-bridge-db`, `divine-bridge-types`, and `divine-video-worker`; planned follow-on services should land beside them in `crates/`, not in `.claude/worktrees/`. Operational config lives in `config/`, SQL migrations in `migrations/`, local PDS assets in `deploy/pds/`, and canonical planning or runbook material in `docs/`.

## Build, Test, and Development Commands

Use `cargo check --workspace` for a fast compile pass and `bash scripts/test-workspace.sh` for the full baseline verification suite. Package-level checks are `cargo test -p divine-atbridge` and `cargo test -p divine-video-worker`. Start local infra with `docker compose -f config/docker-compose.yml up -d`; `docs/runbooks/dev-bootstrap.md` documents the required `libpq` setup before Diesel-linked tests.

## Coding Style & Naming Conventions

Rust code should follow `rustfmt` defaults and idiomatic snake_case naming for modules, files, and tests. Use lowercase, hyphenated names for Markdown docs such as `docs/runbooks/dev-bootstrap.md`. Keep Markdown and YAML indentation consistent with two spaces, and avoid introducing a second build or lint path that bypasses the Cargo workspace.

## Testing Guidelines

Prefer behavior-focused Rust test names such as `translates_nip71_video_to_bsky_post` or `deletion_event_deletes_record`. Any non-trivial bridge, provisioning, replay, or media-path change should land with unit or integration coverage in the owning crate. Treat missing tests for new functionality as incomplete work, not follow-up.

## Commit & Pull Request Guidelines

This branch has no commit history yet, so there is no repository-specific commit convention to copy. Start with Conventional Commit prefixes such as `feat:`, `fix:`, and `docs:` in the imperative mood. Pull requests should summarize scope, list any new commands or config files, link the relevant section of `pompt_plan.md`, and include logs or screenshots when behavior changes.

## Agent-Specific Notes

Keep transient agent output, logs, and experiments inside `.claude/` or ignored files. `.claude/worktrees/` is scratch space, not the source of truth; promote code into the root workspace before treating it as project state. When repo layout or commands change, update this guide and the matching runbook in `docs/runbooks/` in the same change.
