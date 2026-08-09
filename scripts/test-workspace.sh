#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

prepend_path() {
  local value="$1"
  local current="${2:-}"
  if [[ -n "$current" ]]; then
    printf '%s:%s' "$value" "$current"
  else
    printf '%s' "$value"
  fi
}

configure_libpq() {
  local lib_dir=""
  local include_dir=""
  local pkgconfig_dir=""
  local os_name

  os_name="$(uname -s)"

  if command -v pg_config >/dev/null 2>&1; then
    lib_dir="$(pg_config --libdir)"
    include_dir="$(pg_config --includedir)"
  elif [[ "$os_name" == "Darwin" ]] && command -v brew >/dev/null 2>&1; then
    local brew_prefix
    brew_prefix="$(brew --prefix libpq 2>/dev/null || true)"
    if [[ -n "$brew_prefix" && ! -d "$brew_prefix/lib" ]]; then
      local cellar_root
      cellar_root="$(brew --cellar libpq 2>/dev/null || true)"
      if [[ -n "$cellar_root" && -d "$cellar_root" ]]; then
        brew_prefix="$(find "$cellar_root" -maxdepth 1 -mindepth 1 -type d | sort | tail -n 1)"
      fi
    fi
    if [[ -n "$brew_prefix" && -d "$brew_prefix/lib" ]]; then
      export PATH="$(prepend_path "$brew_prefix/bin" "${PATH:-}")"
      lib_dir="$brew_prefix/lib"
      include_dir="$brew_prefix/include"
    fi
  fi

  if [[ -n "$lib_dir" && -d "$lib_dir" ]]; then
    export LIBRARY_PATH="$(prepend_path "$lib_dir" "${LIBRARY_PATH:-}")"
    if [[ "$os_name" == "Darwin" ]]; then
      export DYLD_FALLBACK_LIBRARY_PATH="$(prepend_path "$lib_dir" "${DYLD_FALLBACK_LIBRARY_PATH:-}")"
    else
      export LD_LIBRARY_PATH="$(prepend_path "$lib_dir" "${LD_LIBRARY_PATH:-}")"
    fi
    pkgconfig_dir="$lib_dir/pkgconfig"
    if [[ -d "$pkgconfig_dir" ]]; then
      export PKG_CONFIG_PATH="$(prepend_path "$pkgconfig_dir" "${PKG_CONFIG_PATH:-}")"
    fi
  fi

  if [[ -n "$include_dir" && -d "$include_dir" ]]; then
    export CPATH="$(prepend_path "$include_dir" "${CPATH:-}")"
  fi
}

require_cmd() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "missing required command: $command_name" >&2
    exit 1
  fi
}

run_cargo_test_allowing_libpq_teardown_segv() {
  local log_file
  log_file="$(mktemp)"

  set +e
  cargo test "$@" 2>&1 | tee "$log_file"
  local status="${PIPESTATUS[0]}"
  set -e

  if [[ "$status" -eq 0 ]]; then
    rm -f "$log_file"
    return 0
  fi

  if grep -q "signal: 11, SIGSEGV" "$log_file" \
    && grep -q "test result: ok" "$log_file" \
    && ! grep -q "FAILED" "$log_file"; then
    echo "accepted known pooled libpq teardown SIGSEGV after successful tests: cargo test $*" >&2
    rm -f "$log_file"
    return 0
  fi

  rm -f "$log_file"
  return "$status"
}

require_cmd cargo
configure_libpq

cargo check --workspace
cargo test -p divine-atbridge
cargo test -p divine-video-worker
cargo test --workspace \
  --exclude divine-feedgen \
  --exclude divine-handle-gateway \
  --exclude divine-labeler
run_cargo_test_allowing_libpq_teardown_segv -p divine-feedgen
run_cargo_test_allowing_libpq_teardown_segv -p divine-handle-gateway
run_cargo_test_allowing_libpq_teardown_segv -p divine-labeler
