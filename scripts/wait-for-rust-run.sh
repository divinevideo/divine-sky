#!/usr/bin/env bash
set -euo pipefail

# Blocks until the Rust workflow has passed for one commit, so the Docker
# workflow never publishes an image built from untested code.
#
# rust.yml only runs on `push` to main and on pull_request, so the only commits
# with a `push` run are the ones that landed on main. A commit that has no run
# at all will never grow one, so a short grace period is enough before failing
# it; waiting out the whole window would hide a bad ref behind a timeout.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${1:-}" == "--self-test" ]]; then
  exec bash "$repo_root/scripts/tests/wait-for-rust-run.sh"
fi

repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
head_sha="${HEAD_SHA:?HEAD_SHA is required}"
attempts="${WAIT_ATTEMPTS:-120}"
interval="${WAIT_INTERVAL_SECONDS:-15}"
missing_limit="${MISSING_RUN_ATTEMPTS:-8}"

missing=0
for ((attempt = 1; attempt <= attempts; attempt++)); do
  if ! runs="$(
    gh run list \
      --repo "$repo" \
      --workflow rust.yml \
      --commit "$head_sha" \
      --event push \
      --json conclusion,status
  )"; then
    echo "Could not list Rust runs for ${head_sha} (attempt ${attempt}/${attempts}); retrying."
    sleep "$interval"
    continue
  fi

  conclusion="$(jq -r 'map(select(.status == "completed")) | .[0].conclusion // ""' <<<"$runs")"

  case "$conclusion" in
    success)
      echo "Rust workflow passed for ${head_sha}."
      exit 0
      ;;
    "") ;;
    *)
      echo "::error::Rust workflow concluded '${conclusion}' for ${head_sha}; refusing to publish images."
      exit 1
      ;;
  esac

  if [[ "$(jq -r 'length' <<<"$runs")" -eq 0 ]]; then
    missing=$((missing + 1))
    if ((missing >= missing_limit)); then
      echo "::error::No Rust workflow run on main for ${head_sha}; refusing to publish images. Only a commit that has landed on main can be published."
      exit 1
    fi
  else
    missing=0
  fi

  echo "Waiting for Rust workflow to complete for ${head_sha} (attempt ${attempt}/${attempts})."
  sleep "$interval"
done

echo "::error::Timed out waiting for Rust workflow to pass for ${head_sha}; refusing to publish images."
exit 1
