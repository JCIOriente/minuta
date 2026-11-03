# npm distribution — Implementation Plan

- Spec: [docs/specs/2026-11-03-npm-distribution.md](../specs/2026-11-03-npm-distribution.md)
- Date: 2026-11-03
- Status: approved

## Overview
Five tasks, cut along file boundaries. The launcher and its platform table come first because the build script and both workflows consume them. After T1 and T2, the packaging job (T3) and the release workflow (T4) run in parallel because they own different workflow files. The docs (T5) touch no code and can start at once. Nothing in this plan publishes to npm: T4 only builds the workflow, and the maintainer runs the first release.

## Contract
`npm/minuta/lib/platform.js` is the single platform table (package name, Rust target). T1 owns it. T2, T3 and T4 read it and never repeat the table.

## Impact
n/a. No Rust code, schema or runtime data format changes; `make ci` stays Rust-only.

## Dependency graph
```
T1 ──> T2 ──> T3
        └───> T4
T5 (independent)
```
Can start immediately in parallel: T1, T5. After T2 merges: T3, T4.

## Tasks

### T1: Platform table and launcher
- Priority: 1
- Depends on: none
- Owns: `npm/minuta/**`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] `lib/platform.js` maps darwin/arm64, darwin/x64, linux/x64, linux/arm64 to the packages and Rust targets in the spec table
  - [ ] on `win32` the launcher prints the WSL2 message with the `wsl --install` hint and exits 1
  - [ ] on an unlisted platform or arch it prints platform and arch, suggests `cargo install --locked minuta`, and exits 1
  - [ ] with the platform package missing it names the package and the reinstall command, and exits 1
  - [ ] the launcher passes arguments and stdio through, exits with the binary's code, and forwards SIGINT, SIGTERM and SIGHUP to the child (re-raising the signal that ended it)
  - [ ] `package.json` has `bin`, `engines.node`, `license` Apache-2.0, no `scripts.postinstall`, and the four platform packages in `optionalDependencies`
  - [ ] `node --test npm/minuta/test` passes with no dependencies installed
- Tests first: `platform_maps_every_supported_pair`, `platform_rejects_unknown_pair`, `win32_message_points_to_wsl2`, `missing_package_message_names_package`, `launcher_passes_args_and_exit_code`, `launcher_passes_stdout_and_stderr`, `launcher_forwards_sigterm_to_child`
- Notes: the launcher test uses a fake platform package whose "binary" is a shell script. `optionalDependencies` versions are placeholders until T2 stamps them; the test for exact pinning lives in T2.

### T2: build-packages script
- Priority: 1
- Depends on: T1
- Owns: `npm/scripts/**`
- Touches (read-only or additive): `npm/minuta/package.json` (read), `npm/minuta/lib/platform.js` (read)
- Acceptance:
  - [ ] given a version and a binary path per target, it writes four staging packages with `package.json` (`os`, `cpu`, `files`, `repository`, license Apache-2.0, same version) and `bin/minuta` at mode 0755
  - [ ] it writes the main package with its version and every optional dependency pinned to that exact version (no range)
  - [ ] it fails when the version differs from `Cargo.toml`, or a binary path is missing
  - [ ] output is generated into a given directory; nothing platform-specific is committed
  - [ ] `node --test npm/scripts` passes
- Tests first: `writes_platform_package_with_os_and_cpu`, `binary_is_mode_0755`, `optional_dependencies_are_exact`, `rejects_version_not_matching_cargo_toml`, `rejects_missing_binary`
- Notes: Node built-ins only. Take the package list from `lib/platform.js`.

### T3: Packaging CI job
- Priority: 2
- Depends on: T2
- Owns: `.github/workflows/ci.yml`
- Touches (read-only or additive): `npm/**` (read)
- Acceptance:
  - [ ] a `node` job runs `node --test` for `npm/minuta/test` and `npm/scripts` on pull requests
  - [ ] a `packaging` job (ubuntu and macOS, paths `npm/**`) builds the debug binary, runs `build-packages.mjs`, `npm pack`s all five packages, installs the tarballs into an empty directory, and asserts `minuta --version` equals the `Cargo.toml` version
  - [ ] the existing `ci` job and `make ci` are unchanged
- Tests first: the job itself; prove it fails by temporarily dropping `files` from a generated package, and say so in the PR
- Notes: keep the Rust job free of Node. Path filter goes on the new jobs only.

### T4: Release workflow
- Priority: 2
- Depends on: T2
- Owns: `.github/workflows/release.yml`
- Touches (read-only or additive): `npm/scripts/build-packages.mjs` (read)
- Acceptance:
  - [ ] triggers on push of tags `v*` and on `workflow_dispatch` with a `dry_run` input
  - [ ] `verify` fails when the tag without `v` differs from the `Cargo.toml` version
  - [ ] `build` matrix of four targets runs `cargo build --release --locked --target <t>`; both Linux targets are musl and the job fails if `file` does not report static linking
  - [ ] `publish` builds the packages, publishes the four platform packages first and the main package last with `--access public --provenance`, authenticated by the `NPM_TOKEN` secret
  - [ ] a package whose exact version is already on the registry is skipped (`npm view <pkg>@<v> version`), so a re-run is safe
  - [ ] with `dry_run` (and never from a branch or pull request) it uses `npm publish --dry-run` and publishes nothing
  - [ ] the workflow has no job that can publish from a pull request
- Tests first: `actionlint` on the file; a `workflow_dispatch` dry run on the PR branch, with its link in the PR
- Notes: x86_64 musl uses `musl-tools`; aarch64 needs a cross C compiler for `ring` (spec, open question 2). Add `id-token: write` for provenance. The maintainer creates the `NPM_TOKEN` secret and the first tag; this task does neither.

### T5: Docs in English and Spanish
- Priority: 3
- Depends on: none
- Owns: `README.md`, `README.es.md`, `docs/install.md`, `docs/install.es.md`
- Touches (read-only or additive): none
- Acceptance:
  - [ ] each of the four files has an npm install path (`npm i -g @jcioriente/minuta`, `npx @jcioriente/minuta doctor`) next to `cargo install`
  - [ ] each says Windows uses WSL2 and that ffmpeg, whisper.cpp and an LLM backend are still installed separately
  - [ ] English and Spanish say the same things; the supported platforms match the spec table
- Tests first: n/a (docs); check links and that `minuta doctor` is still the next step
- Notes: follow the wording and structure already used in each file. Merge it close to the first release, since the package does not exist before then.

## Coverage check
| Spec requirement | Task(s) |
|---|---|
| Launcher, exit status, signal forwarding | T1 |
| Windows message, unlisted platform message, missing package message | T1 |
| Platform table in one place | T1 |
| Platform packages, versions equal `Cargo.toml`, exact pins, mode 0755 | T2 |
| Tag and `Cargo.toml` agree before publishing | T4 (verify), T2 (script check) |
| Four builds, static musl Linux | T4 |
| Publish order, provenance, `NPM_TOKEN`, re-run safety | T4 |
| Dry-run input | T4 |
| CI proves install on every PR touching `npm/` | T3 |
| `make ci` stays Rust-only, Node tests in their own job | T3 |
| Docs in English and Spanish, WSL2 note | T5 |
