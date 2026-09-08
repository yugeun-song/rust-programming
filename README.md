# rust-programming

A study archive for the Rust language, organized by topic. Each topic is a crate
at the repository root, and each program in it runs on its own. The build is
tuned so a debug binary reads next to its disassembly.

## Layout

```text
Cargo.toml              [workspace] members, profiles
.cargo/config.toml      codegen flags
<topic>/
    Cargo.toml          three lines, inherits the workspace
    src/lib.rs          code shared by the topic's programs, optional
    src/bin/<name>.rs   one program
    src/bin/<name>/     one program that needs more than one file
```

Topic crates sit at the root and are listed in `members`, the way tokio and serde
lay out their workspaces. Inside a topic the `src/bin` convention applies, so a
new program is just a new file, or a directory with `main.rs` when it needs more
than one; a new topic needs a three-line `Cargo.toml` and one line in `members`,
which `./scaffold.sh <topic> [program ...]` writes.

Binary names must be unique across topics, since every target lands in
`target/debug/`. Profiles work only in the root manifest: cargo warns about and
ignores a `[profile]` in a member. `.project.toml` holds the topic roadmap.

## Build and run

```sh
cargo build                    # debug
cargo build --release          # optimized, still debuggable
cargo run -p format --bin hello_world
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
ls */src/bin/                  # every program, by topic
```

Neither profile needs an argument or an environment variable.

```sh
cargo rustc -p format --bin hello_world -- --emit asm   # assembly
cargo expand -p format --bin hello_world                # macro expansion
```

## Profiles

Debug keeps source and assembly in step: `opt-level = 0`, `codegen-units = 1`,
`incremental = false`, `debug = "full"`, unstripped, with debug assertions and
overflow checks on. The flags in `.cargo/config.toml` add `link-dead-code`, so
uncalled functions still reach the debug binary, though release `lto = "thin"`
drops them anyway; `relocation-model=static`, so objdump
addresses are the ones gdb, perf and valgrind report at runtime;
`force-frame-pointers=yes` for accurate unwinding; and `dwarf-version=5`.

Release adds `lto = "thin"` and `codegen-units = 1`, the usual production tuning,
but keeps debug info, symbols and frame pointers so perf and gdb still work.

Three limits are worth knowing. `#[inline(always)]` is folded even at
`opt-level = 0`; use `#[inline(never)]`, or a one-off `RUSTFLAGS` that repeats
the four flags from `.cargo/config.toml` and adds `-C no-prepopulate-passes`,
since `RUSTFLAGS` replaces `build.rustflags` instead of extending it. Confine
that one-off to debug: next to `link-dead-code` the flag leaves the release
link with undefined symbols, so it cannot be permanent.
Macros expand before code generation and leave nothing in DWARF, so read them
with `cargo expand`. Sanitizers and `cargo fuzz` need nightly, so on stable use
valgrind for leaks and memory errors.

## Requirements

rustup with the stable toolchain, plus a C toolchain for `cc`, which rustc calls
as the linker driver; gcc and clang both work. `rust-toolchain.toml` installs
`rustfmt`, `clippy`, `rust-src` and `rust-analyzer` on first use. Nothing else
is required.

Optional: `gdb` and `lldb` behind `rust-gdb` and `rust-lldb`, `binutils` for
`objdump -dS --demangle` and `readelf`, `perf`, `strace`, `ltrace`, `valgrind`,
`cargo-expand`, `cargo-show-asm`, `mold`, `sccache`.
