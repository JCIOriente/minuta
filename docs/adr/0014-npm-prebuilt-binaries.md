# ADR-0014: npm distribution as prebuilt binaries in per-platform packages

- Status: accepted
- Date: 2026-11-03
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-11-03-npm-distribution.md, ADR-0002 (whisper-cli subprocess)

## Context
Installing minuta needs a Rust toolchain. Many target users have Node but not Rust. minuta starts `ffmpeg` and `whisper-cli` as child processes (ADR-0002) and signals process groups, so it cannot run as WASM. Package names and the shape of the packages are hard to change once users install them.

## Options considered
1. **Main package plus one package per platform (`optionalDependencies`)**, with a small JS launcher: no install scripts, works offline from a mirror, npm picks the right binary through `os`/`cpu`. Cost: five packages to publish in order.
2. **One package with all binaries inside**: simplest publish, but every install downloads every platform (several times the size).
3. **`postinstall` that downloads the binary from GitHub releases**: one package, but breaks with `--ignore-scripts`, proxies and offline mirrors.
4. **WASM or `napi-rs` bindings**: WASM cannot spawn processes or send signals; `napi-rs` keeps the external-tool dependencies and adds a native-addon build matrix for no gain to a CLI.

## Decision
Option 1, under the `@jcioriente` scope: `@jcioriente/minuta` (launcher) and `@jcioriente/minuta-<os>-<arch>` for darwin-arm64, darwin-x64, linux-x64 and linux-arm64. Windows users use WSL2, which npm treats as Linux. All packages share the `Cargo.toml` version and the main package pins them exactly.

## Consequences
- Easier: install with npm or npx without Rust; no install scripts; small downloads.
- Harder: each release publishes five packages in order; the Linux builds must be static (musl) or have a documented glibc floor.
- Must do: a release workflow, packaging tests in CI, and docs in English and Spanish.
- Revisit if: native Windows is supported (add `win32-x64`), or a Homebrew or curl installer makes npm redundant.
