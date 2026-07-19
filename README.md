# rust_programming

A personal study workspace for the Rust language. It is a single Cargo package:
each topic program is a small file at `src/bin/<theme>/<name>.rs`, registered as
an independent binary and run with `cargo run --bin <theme>_<name>`. A learning
archive rather than a shipped library or application.

## Layout

```text
.
├── .project.toml            Single source of truth (build, style, naming, layout, debug, taxonomy).
├── Cargo.toml               Package manifest, lints, profiles, and the [[bin]] registry.
├── .cargo/config.toml       Build flags, cargo aliases, and optional linker blocks.
├── rust-toolchain.toml      Pinned toolchain channel and components.
├── src/bin/<theme>/<name>.rs  One topic program; registered as binary <theme>_<name>.
└── scripts/                 setup, check, create_project.
```

Each theme is a folder under `src/bin/` that may hold **several** independent
programs, one per file. Every program is declared explicitly as a `[[bin]]` in
`Cargo.toml` — this is required because Cargo's auto-discovery allows only one
binary per `src/bin` subfolder, so it cannot group multiple independent binaries
in a theme folder. This is exactly how the official `rustlings` project registers
each exercise. Source lives under `src/`; compiled binaries land in the
Cargo-managed, gitignored `target/` directory, so source and binaries stay
separated. Because this remains a real Cargo package, `clippy`, `rustfmt`,
`rust-analyzer`, and `cargo test` all keep working. The planned topic taxonomy is
recorded in `.project.toml`; topic programs are created on demand.

If a single program grows, organize its own folder with sibling module files
(`main.rs` plus `mod` files), the way Tokio organizes `src/net/tcp/`.

## Requirements

- Rust stable 1.96 or newer, edition 2024, installed via rustup.
- Target `x86_64-unknown-linux-gnu`.

Run `scripts/setup.sh` once to add the toolchain components declared in
`rust-toolchain.toml` (`rust-src`, `rust-analyzer`, `rustfmt`, `clippy`).

## Build and run

Cargo is controlled per target — you never need to build the whole set:

```sh
cargo run   --bin hello_world       # build + run one program
cargo build --bin hello_world       # build just that program
cargo check --bin hello_world       # type-check one program, no codegen (fastest)
cargo build                         # build every binary (incremental: unchanged
                                    #   targets are fingerprint-skipped, not rebuilt)
scripts/check.sh                    # fmt + clippy + build + test
```

Aliases (`.cargo/config.toml`): `cargo b` build, `cargo t` test, `cargo l`
clippy at deny-warnings, `cargo topic hello_world` run that program.

## Add a new topic

```sh
scripts/create_project.sh <theme> <name>
```

For example, `scripts/create_project.sh memory ownership` creates
`src/bin/memory/ownership.rs` and registers the `[[bin]]` `memory_ownership`, run
with `cargo run --bin memory_ownership`. Both arguments must be snake_case. When
`<name>` is `main`, the binary is named after the theme alone (as with
`hello_world`).

## Debugging and profiling

All profiles keep full debug info (`debug = 2`); `release` and `perf` also keep
symbols (`strip = false`). `.cargo/config.toml` sets
`-C symbol-mangling-version=v0` (readable Rust symbols) and
`-C force-frame-pointers=yes` (reliable call graphs). Binaries land at
`target/debug/<theme>_<name>` (or under `target/perf/` for the perf profile). The
examples below use a `memory_ownership` topic.

- gdb: `rust-gdb target/debug/memory_ownership`. `rust-gdb` loads Rust
  pretty-printers. Break on demangled names (`break memory_ownership::main`), then
  `run`, `next`, `step`, `print <var>`, `backtrace`. With the `rust-src`
  component, step into std after remapping the source path:
  `set substitute-path /rustc/<hash> <sysroot>/lib/rustlib/src/rust`
  (`rustc --version --verbose` gives the commit hash; `rustc --print sysroot`
  gives the base).
- lldb: `rust-lldb target/debug/memory_ownership` (needs the `lldb` package):
  `breakpoint set --name memory_ownership::main`, `run`, `next`, `step`,
  `frame variable`, `bt`.
- perf: build the profiling profile, then sample with call graphs:
  ```sh
  cargo build --profile perf --bin memory_ownership
  perf record -g target/perf/memory_ownership
  perf report                    # readable symbols
  perf annotate <symbol>         # source interleaved with disassembly
  ```
  Frame pointers keep call graphs cheap and accurate. If sampling is blocked,
  lower `kernel.perf_event_paranoid` (sudo); for kernel symbols also
  `kernel.kptr_restrict=0`.
- strace / ltrace: `strace ./target/debug/memory_ownership` traces syscalls;
  `ltrace ...` traces libc calls.
- valgrind: `valgrind ./target/debug/memory_ownership` for memcheck, or
  `valgrind --tool=callgrind ...` for call profiling.
- Optional speedups: enable mold or lld via the commented block in
  `.cargo/config.toml` (repeat the two rustflags there), and cache compilation
  with `export RUSTC_WRAPPER=sccache`.

## Operations

```text
scripts/setup.sh           Add required toolchain components (idempotent).
scripts/check.sh           Format check, lint, build, and test in sequence.
scripts/create_project.sh  Scaffold a topic (src/bin/<theme>/<name>.rs) and register its [[bin]].
```

## Documentation

- `.project.toml` is the authoritative configuration and policy record. It also
  records the layout, the naming and style rules, the debug configuration, and
  the planned topic taxonomy.

All repository files are English only.
