#!/usr/bin/env bash
# Add the toolchain components required for debugging and IDE support.
# Idempotent: `rustup component add` is a no-op when a component is already
# present. This script never installs system packages and never uses sudo; it
# only prints suggestions for optional tooling.
set -euo pipefail

# Resolve the repository root so the script works from any directory and so
# rustup applies to the toolchain pinned by rust-toolchain.toml.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

if ! command -v rustup >/dev/null 2>&1; then
    echo "error: rustup is not installed; install the Rust toolchain first." >&2
    exit 1
fi

echo "Adding required toolchain components (rust-src, rust-analyzer)..."
rustup component add rust-src rust-analyzer

echo
echo "Required components are in place."
echo
echo "Optional tooling (not installed by this script):"
echo
echo "  cargo extensions (install with 'cargo install <name>'):"
echo "    cargo-nextest   faster, richer test runner"
echo "    cargo-expand    show macro-expanded source"
echo "    taplo-cli       TOML formatter and linter"
echo
echo "  system packages (install with your package manager, for example pacman):"
echo "    mold                 fast linker (opt in via .cargo/config.toml)"
echo "    sccache              compiler cache (opt in via RUSTC_WRAPPER=sccache)"
echo "    lldb                 LLVM debugger backing the rust-lldb front end"
echo "    perf strace ltrace   profiling and syscall tracing"
echo
echo "These are suggestions only; the build never requires them."
