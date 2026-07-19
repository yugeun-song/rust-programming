#!/usr/bin/env bash
# Scaffold a new runnable topic program and register it as a [[bin]] target.
#
# Usage:
#     scripts/create_project.sh <theme> <name>
#
# Creates src/bin/<theme>/<name>.rs and appends a [[bin]] entry to Cargo.toml, so
# it becomes an independent binary. A single theme folder can hold several such
# programs. The binary name is <theme>_<name> (or <theme> when <name> is "main").
# Run it with: cargo run --bin <binary>. Both arguments must be snake_case.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

usage() {
    echo "usage: scripts/create_project.sh <theme> <name>" >&2
    echo "  both arguments must be snake_case (start with a letter; a-z, 0-9, _)" >&2
}

if [[ $# -ne 2 ]]; then
    usage
    exit 2
fi

THEME="$1"
NAME="$2"

snake_case_re='^[a-z][a-z0-9]*(_[a-z0-9]+)*$'
for arg in "${THEME}" "${NAME}"; do
    if [[ ! "${arg}" =~ ${snake_case_re} ]]; then
        echo "error: '${arg}' is not snake_case" >&2
        usage
        exit 2
    fi
done

if [[ "${NAME}" == "main" ]]; then
    BIN="${THEME}"
else
    BIN="${THEME}_${NAME}"
fi

DIR="src/bin/${THEME}"
FILE="${DIR}/${NAME}.rs"

if [[ -e "${FILE}" ]]; then
    echo "error: ${FILE} already exists; refusing to overwrite" >&2
    exit 1
fi
if grep -q "^name = \"${BIN}\"\$" Cargo.toml; then
    echo "error: a [[bin]] named '${BIN}' is already registered in Cargo.toml" >&2
    exit 1
fi

mkdir -p "${DIR}"
cat > "${FILE}" <<EOF
fn main() {
    println!("${THEME}/${NAME}: not yet implemented");
}
EOF

# Register the binary: insert a [[bin]] block just before the end marker so all
# entries stay grouped together in Cargo.toml. awk prints the block with explicit
# newlines (a $(...) capture would strip the trailing blank line and merge into
# the marker).
awk -v bin="${BIN}" -v path="${FILE}" '
    /^# --- end topic binaries ---$/ {
        print "[[bin]]"
        print "name = \"" bin "\""
        print "path = \"" path "\""
        print ""
    }
    { print }
' Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml

echo "created ${FILE}"
echo "registered [[bin]] ${BIN} -> ${FILE} in Cargo.toml"
echo
echo "Next steps:"
echo "  1. Implement ${FILE}."
echo "  2. Run:    cargo run --bin ${BIN}"
echo "  3. Verify: scripts/check.sh"
