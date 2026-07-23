# Repository Guidelines

## Divine Context And Brain

Before broad product, architecture, protocol, cross-repo, service-boundary, or pull-request authoring, review, or modification work, read the shared Divine context primer.

Resolve the context directory and clone it there if it is missing:

```bash
CONTEXT_DIR="${DIVINE_CONTEXT_ROOT:-../divine-context}"
[ -e "$CONTEXT_DIR/.git" ] || gh repo clone divinevideo/divine-context "$CONTEXT_DIR"
```

Use that value as `<context-dir>` below.

The `divine-context` repo is private, so cloning requires GitHub access. If clone, network, or auth fails, continue from the local repo docs and avoid cross-repo assumptions.

Before updating an existing context checkout, verify it is clean and on its default branch. If it is clean and on the default branch, update it with `git -C <context-dir> pull --ff-only`. If it is dirty, on another branch, cannot fast-forward, or network/auth fails, leave it untouched and say the context may be stale.

Read `<context-dir>/AGENT_CONTEXT.md` and follow its instructions. If unavailable, continue from the local repo docs and avoid cross-repo assumptions.

Before working on a pull request, follow `<context-dir>/PR_REVIEW.md` and use `<context-dir>/PR_REVIEW_TEAMS.md` to request the normal team and check takeover authority. Ordinary review remains open to any eligible Divine human. Before modifying a pull-request branch, enforce the mapping and every takeover gate; if the mapping cannot be read, feedback-only review may continue but automated takeover must stop. Request and verify required human review automatically when tooling permits. If the runbook is unavailable, leave the pull request open and report the blocker.

If a Divine Brain search or ask tool is available, you may use it for company memory. Treat it as optional and credentialed: tool names vary by client, and work must continue when Brain is unavailable. When Brain results influence work, cite the returned document ids. Never commit Brain credentials or expose Brain-derived sensitive content in public PRs, issues, branch names, commit messages, code comments, logs, screenshots, release notes, or externally shared agent transcripts.

## Project Structure & Module Organization

The Rust workspace lives at the repository root in `crates/`. Current crates are `divine-atbridge`, `divine-bridge-db`, `divine-bridge-types`, and `divine-video-worker`; planned follow-on services should land beside them in `crates/`, not in `.claude/worktrees/`. Operational config lives in `config/`, SQL migrations in `migrations/`, local PDS assets in `deploy/pds/`, and canonical planning or runbook material in `docs/`.

## Build, Test, and Development Commands

Use `cargo check --workspace` for a fast compile pass and `bash scripts/test-workspace.sh` for the full baseline verification suite. Package-level checks are `cargo test -p divine-atbridge` and `cargo test -p divine-video-worker`. Start local infra with `docker compose -f config/docker-compose.yml up -d`; `docs/runbooks/dev-bootstrap.md` documents the required `libpq` setup before Diesel-linked tests.

## Coding Style & Naming Conventions

Rust code should follow `rustfmt` defaults and idiomatic snake_case naming for modules, files, and tests. Use lowercase, hyphenated names for Markdown docs such as `docs/runbooks/dev-bootstrap.md`. Keep Markdown and YAML indentation consistent with two spaces, and avoid introducing a second build or lint path that bypasses the Cargo workspace.

## Testing Guidelines

Prefer behavior-focused Rust test names such as `translates_nip71_video_to_bsky_post` or `deletion_event_deletes_record`. Any non-trivial bridge, provisioning, replay, or media-path change should land with unit or integration coverage in the owning crate. Treat missing tests for new functionality as incomplete work, not follow-up.

## Commit & Pull Request Guidelines

Use Conventional Commit PR titles in the form `type(scope): summary` or `type: summary`, and set the correct title when opening the PR instead of relying on a later edit. If the title changes after opening, verify that the semantic PR title check reruns successfully. Pull requests should summarize scope, list any new commands or config files, link the relevant section of `pompt_plan.md`, and include logs or screenshots when behavior changes.

Keep PRs tightly scoped. Do not mix unrelated cleanup, formatting churn, or speculative refactors into the same change. Temporary or transitional code must include `TODO(#issue):` with the tracking issue for removal.

## Sensitive Information

Do not publish private credentials, internal-only environment details, or sensitive user data. Public issues, PRs, branch names, screenshots, and descriptions must not mention corporate partners, customers, brands, campaign names, or other sensitive external identities unless a maintainer explicitly approves it. Use generic descriptors instead.

## Agent-Specific Notes

Keep transient agent output, logs, and experiments inside `.claude/` or ignored files. `.claude/worktrees/` is scratch space, not the source of truth; promote code into the root workspace before treating it as project state. When repo layout or commands change, update this guide and the matching runbook in `docs/runbooks/` in the same change.
