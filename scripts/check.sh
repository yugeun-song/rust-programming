#!/usr/bin/env bash
# Full local verification: format check, lint, build, test.
# Any failing step aborts the run (set -e).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

echo "==> rustfmt (check)"
cargo fmt --all -- --check

echo "==> clippy (deny warnings)"
cargo clippy --all-targets -- -D warnings

echo "==> build"
cargo build

echo "==> test"
cargo test

echo "==> all checks passed"
