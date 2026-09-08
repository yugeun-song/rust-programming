#!/usr/bin/env bash
set -euo pipefail
shopt -s nullglob

PROG="${0##*/}"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST="$ROOT/Cargo.toml"
ROADMAP="$ROOT/.project.toml"

usage() {
    cat <<EOF
Usage: $PROG <topic> [program ...] [options]

Creates a topic crate at the repository root, or adds programs to one that is
already there. The manifest is written by hand rather than by cargo new, so a
topic named after a Rust keyword works too: cargo new rejects struct, enum,
trait, async and unsafe, while a hand-written manifest builds them.

Options:
  -d, --dir <name>   a program spanning several files, as src/bin/<name>/main.rs
  -l, --lib          add src/lib.rs, for code shared by the topic's programs
  -n, --dry-run      print the plan and write nothing
  -h, --help         this text

Layout it writes, which is the one README.md documents:

  <topic>/Cargo.toml            three lines, inheriting the workspace edition
  <topic>/src/lib.rs            optional, shared by the topic's programs
  <topic>/src/bin/<name>.rs     one program, needs a main
  <topic>/src/bin/<name>/       one program spanning several files
      main.rs                   the crate root of that program
                                siblings go beside it and need a mod declaration

Every binary target in the workspace lands in target/debug/, so binary names
must be unique across topics. Cargo only warns about a collision and lets one
binary overwrite the other; this script refuses it instead.

Examples:
  $PROG slice borrowed_view chunks
  $PROG lifetime elision --dir bounds --lib
  $PROG borrow --dry-run
EOF
}

die() {
    printf '%s: %s\n' "$PROG" "$1" >&2
    exit 1
}

valid_name() {
    [[ $1 =~ ^[a-z][a-z0-9_]*$ ]]
}

existing_bins() {
    local path
    for path in "$ROOT"/*/src/bin/*.rs; do
        basename "$path" .rs
    done
    for path in "$ROOT"/*/src/bin/*/; do
        basename "$path"
    done
    for path in "$ROOT"/*/src/main.rs; do
        basename "$(dirname "$(dirname "$path")")"
    done
}

in_roadmap() {
    grep -q "name = \"$1\"" "$ROADMAP"
}

register_member() {
    local topic="$1" tmp
    tmp="$(mktemp "$MANIFEST.XXXXXX")"
    awk -v topic="$topic" '
        /^members = \[$/ { print; inside = 1; next }
        inside && /^\]$/ {
            if (!placed) print "    \"" topic "\","
            inside = 0
            print
            next
        }
        inside {
            name = $0
            gsub(/^[[:space:]]*"|",?[[:space:]]*$/, "", name)
            if (!placed && topic < name) {
                print "    \"" topic "\","
                placed = 1
            }
            print
            next
        }
        { print }
    ' "$MANIFEST" >"$tmp"
    mv "$tmp" "$MANIFEST"
}

write_file() {
    local path="$1" body="$2"
    if [ -n "$DRY_RUN" ]; then
        printf '  write  %s\n' "${path#"$ROOT"/}"
        return
    fi
    mkdir -p "$(dirname "$path")"
    printf '%s' "$body" >"$path"
    printf '  write  %s\n' "${path#"$ROOT"/}"
}

TOPIC=""
DRY_RUN=""
WANT_LIB=""
FLAT=()
DIRS=()

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help) usage; exit 0 ;;
        -n|--dry-run) DRY_RUN=1; shift ;;
        -l|--lib) WANT_LIB=1; shift ;;
        -d|--dir)
            [ $# -ge 2 ] || die "--dir needs a name"
            DIRS+=("$2"); shift 2 ;;
        -*) die "unknown option $1, see --help" ;;
        *)
            if [ -z "$TOPIC" ]; then TOPIC="$1"; else FLAT+=("$1"); fi
            shift ;;
    esac
done

[ -n "$TOPIC" ] || { usage >&2; exit 2; }
[ -f "$MANIFEST" ] || die "no Cargo.toml at $ROOT"

valid_name "$TOPIC" || die "topic '$TOPIC' must match ^[a-z][a-z0-9_]*$"

TOPIC_DIR="$ROOT/$TOPIC"
FRESH=""
[ -d "$TOPIC_DIR" ] || FRESH=1

if [ -n "$FRESH" ] && ! in_roadmap "$TOPIC"; then
    printf '%s: warning: %s is not in the .project.toml roadmap\n' "$PROG" "$TOPIC" >&2
fi

if [ ${#FLAT[@]} -eq 0 ] && [ ${#DIRS[@]} -eq 0 ]; then
    if [ -n "$FRESH" ]; then
        FLAT+=("$TOPIC")
    elif [ -z "$WANT_LIB" ]; then
        die "$TOPIC already exists, name a program to add or pass --lib"
    fi
fi

NEW_BINS=("${FLAT[@]}" "${DIRS[@]}")
for name in "${NEW_BINS[@]}"; do
    valid_name "$name" || die "program '$name' must match ^[a-z][a-z0-9_]*$"
done

for i in "${!NEW_BINS[@]}"; do
    for j in "${!NEW_BINS[@]}"; do
        [ "$i" -lt "$j" ] || continue
        [ "${NEW_BINS[$i]}" != "${NEW_BINS[$j]}" ] || die "program '${NEW_BINS[$i]}' given twice"
    done
done

TAKEN="$(existing_bins | sort)"
for name in "${NEW_BINS[@]}"; do
    if printf '%s\n' "$TAKEN" | grep -qx "$name"; then
        die "binary name '$name' is already taken in this workspace"
    fi
done

for name in "${FLAT[@]}"; do
    [ ! -e "$TOPIC_DIR/src/bin/$name.rs" ] || die "$TOPIC/src/bin/$name.rs already exists"
done
for name in "${DIRS[@]}"; do
    [ ! -e "$TOPIC_DIR/src/bin/$name" ] || die "$TOPIC/src/bin/$name already exists"
done
if [ -n "$WANT_LIB" ] && [ -e "$TOPIC_DIR/src/lib.rs" ]; then
    die "$TOPIC/src/lib.rs already exists"
fi

[ -z "$DRY_RUN" ] || printf '%s: dry run, nothing is written\n' "$PROG"
printf '%s %s\n' "$([ -n "$FRESH" ] && echo 'creating topic' || echo 'extending topic')" "$TOPIC"

if [ -n "$FRESH" ]; then
    write_file "$TOPIC_DIR/Cargo.toml" "[package]
name = \"$TOPIC\"
edition.workspace = true
"
fi

[ -z "$WANT_LIB" ] || write_file "$TOPIC_DIR/src/lib.rs" "
"

for name in "${FLAT[@]}"; do
    write_file "$TOPIC_DIR/src/bin/$name.rs" "fn main() {}
"
done

for name in "${DIRS[@]}"; do
    write_file "$TOPIC_DIR/src/bin/$name/main.rs" "fn main() {}
"
done

if [ -n "$FRESH" ]; then
    if [ -n "$DRY_RUN" ]; then
        printf '  edit   Cargo.toml, adding "%s" to members\n' "$TOPIC"
    else
        register_member "$TOPIC"
        printf '  edit   Cargo.toml, added "%s" to members\n' "$TOPIC"
    fi
fi

printf '\nnext:\n'
for name in "${NEW_BINS[@]}"; do
    printf '  cargo run -p %s --bin %s\n' "$TOPIC" "$name"
done
printf '  cargo clippy --workspace --all-targets -- -D warnings\n'
