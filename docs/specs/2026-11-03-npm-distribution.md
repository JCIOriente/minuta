# npm distribution — Spec

- Status: draft
- Date: 2026-11-03
- Owner: Arnold Gandarillas
- Related: [ADR-0014](../adr/0014-npm-prebuilt-binaries.md), docs/brief.md:161 (distribution goal: "cargo install, Homebrew, prebuilt binaries")

## TL;DR
- minuta gets published to npm as `@jcioriente/minuta`, so `npm i -g @jcioriente/minuta` or `npx @jcioriente/minuta` works without a Rust toolchain.
- The package ships the compiled Rust binary, not WASM: minuta spawns `ffmpeg` and `whisper-cli` and signals process groups, which WASM cannot do (`Command::new` in `src/tools.rs:322` and `:367`, `process_group(0)` at `:182`, `libc::kill(-group, SIGKILL)` at `:217`; `libc` in `Cargo.toml`).
- One small package per platform holds the binary (`darwin-arm64`, `darwin-x64`, `linux-x64`, `linux-arm64`); the main package is a JS launcher that depends on all of them as optional dependencies, so npm installs only the one that fits.
- Windows users run it in WSL2, where npm sees Linux and installs the Linux binary. Native Windows is out of scope.
- A tag `vX.Y.Z` triggers a workflow that builds the four binaries and publishes the five packages.

## Change in a nutshell
```console
$ npx @jcioriente/minuta doctor        # no Rust, no cargo
$ npm i -g @jcioriente/minuta
$ minuta run standup.mp4 -t summary    # same binary, same behavior as cargo install
```
What npm downloads on an Apple Silicon Mac:
```
@jcioriente/minuta                 JS launcher (bin/minuta.js), ~few KB
@jcioriente/minuta-darwin-arm64    the `minuta` binary
```
The other three platform packages are declared in `optionalDependencies` and skipped because their `os`/`cpu` fields do not match. The launcher finds the binary with `require.resolve`, starts it with the same arguments and stdio, forwards SIGINT, SIGTERM and SIGHUP, and exits with the binary's exit status. There is no `postinstall` script, so `--ignore-scripts` installs work.

## Problem
Installing minuta today needs Rust 1.88 or newer and `cargo install` (README.md:37), which is a large first step for the people minuta is for (meeting recorders, teachers, content creators), many of whom already have Node. Prebuilt binaries are a stated distribution goal (docs/brief.md:161) but nothing builds them yet: `.github/workflows/` has only `ci.yml`.

## Goals / Success criteria
- On macOS (arm64, x64) and Linux (x64, arm64, including WSL2), `npm i -g @jcioriente/minuta && minuta --version` prints the version of the tag, with no Rust installed.
- `npx @jcioriente/minuta doctor` runs the real binary.
- The launcher's exit status equals the binary's, and Ctrl-C during `minuta run` stops the tool and its children the same way as running the binary directly.
- All five packages share the version in `Cargo.toml`; the workflow fails before publishing anything if the tag and `Cargo.toml` disagree.
- On native Windows, the launcher prints a message that points to the WSL2 install steps and exits non-zero.
- A platform with no package gets a message naming the platform and pointing to `cargo install --locked minuta`.
- CI proves the above (see Testing) on every pull request that touches `npm/`.

## Non-goals
- WASM, WASI or `napi-rs` bindings (see FAQ).
- Native Windows binaries (the code is Unix-only today: `libc` is a `cfg(unix)` dependency).
- Bundling ffmpeg, whisper.cpp, models or an LLM backend. `minuta doctor` keeps covering them.
- Homebrew, a curl installer, or binaries attached to the GitHub release.
- A JavaScript API: the package exposes only the `minuta` command.
- Replacing `cargo install`; crates.io publishing stays as it is.

## Users & permissions
| Role | Can | Cannot |
|---|---|---|
| User | install and run through npm | n/a |
| Maintainer (owner of the `jcioriente` npm org) | create the first release, rotate the publish credential | n/a |
| CI release workflow | publish the five packages when a `v*` tag is pushed | publish from a pull request or a branch |

## Tenancy & localization
n/a for data. The docs that gain an npm install path exist in English and Spanish: `README.md`, `README.es.md`, `docs/install.md`, `docs/install.es.md`; both languages are updated together. Launcher messages are English, like minuta's own.

## Domain model
Layout in the repository (new, under `npm/`):
```
npm/
  minuta/                    main package source
    package.json             name @jcioriente/minuta, bin, optionalDependencies, engines.node
    bin/minuta.js            launcher
    lib/platform.js          (platform, arch) -> package name; pure, unit-tested
    test/*.test.js           node:test
  scripts/build-packages.mjs  writes the four platform packages and stamps all versions
```
The platform packages are not committed. `build-packages.mjs` generates each one in a staging directory from a binary path: a `package.json` (`os`, `cpu`, `files`, same version, `repository`, `license` Apache-2.0) and `bin/minuta`.

Platform table (one place, `lib/platform.js`):

| `process.platform` / `arch` | npm package | Rust target |
|---|---|---|
| darwin / arm64 | `@jcioriente/minuta-darwin-arm64` | `aarch64-apple-darwin` |
| darwin / x64 | `@jcioriente/minuta-darwin-x64` | `x86_64-apple-darwin` |
| linux / x64 | `@jcioriente/minuta-linux-x64` | `x86_64-unknown-linux-musl` |
| linux / arm64 | `@jcioriente/minuta-linux-arm64` | `aarch64-unknown-linux-musl` |

Invariants: every package version equals the `Cargo.toml` version; the main package pins each optional dependency to that exact version (no range), so a launcher never runs a binary of another version.

## Interfaces
- **Launcher** (`bin/minuta.js`): resolves the package from the table, runs `child_process.spawn(binary, process.argv.slice(2), { stdio: 'inherit' })`, forwards SIGINT, SIGTERM and SIGHUP to the child, and exits with the child's code (or re-raises the signal that ended it).
- **Release workflow** (`.github/workflows/release.yml`), on push of tags `v*`:
  1. `verify`: tag without the leading `v` equals the `Cargo.toml` version.
  2. `build` (matrix of four targets): `cargo build --release --locked --target <t>`, upload the binary as a workflow artifact.
  3. `publish`: `build-packages.mjs`, then `npm publish --access public --provenance` for the four platform packages, and last the main package, so the main package never points at a missing version.
- **Docs**: an "npm" install path next to `cargo install` in the four docs above; Windows users are told to run it inside WSL2.

## Behavior & edge cases
- Launcher on `win32`: prints "minuta needs WSL2 on Windows. Install it with `wsl --install`, then run `npm i -g @jcioriente/minuta` inside WSL2." with a link to the install guide; exit 1.
- Launcher on an unlisted platform or arch: prints the platform and arch, suggests `cargo install --locked minuta`; exit 1.
- Platform package missing although the platform is listed (install with `--omit=optional`, or a copied `node_modules` from another OS): prints which package is missing and the command to reinstall; exit 1.
- Binary not executable: the build script sets mode 0755, and npm preserves it from the tarball; a launcher test checks the packed file mode.
- Static Linux binaries (musl) run on glibc and musl distributions alike. minuta uses rustls (`Cargo.toml`: `ureq` with `rustls`), so no system OpenSSL is needed. Verified on 2026-11-03 (see Open questions, item 2). The TLS crypto is `ring` (`Cargo.lock`; no `aws-lc`, no `openssl-sys`), which needs a C compiler for the target, so the CI build job installs a musl C toolchain.
- The launcher adds one Node start (tens of ms) to each run; accepted.
- Partial publish (a platform package published, the next fails): re-running the workflow must be safe. `npm publish` of an existing version fails, so the publish step skips a package whose exact version is already on the registry (`npm view <pkg>@<v> version`).

## Data & migrations
None. No runtime data format changes. The first release creates the five packages on the registry (a one-time action by the org owner, see Open questions).

## Testing strategy
- Unit (`node --test`, no dependencies): `lib/platform.js` table for every supported pair, `win32` and an unknown pair; the error message text for each failure case.
- Launcher integration (`node --test`): a fake platform package whose "binary" is a shell script, to check argument passthrough, exit code passthrough, stdout/stderr passthrough, and signal forwarding (SIGTERM reaches the child).
- Packaging (CI, pull requests touching `npm/`): run `build-packages.mjs` with the freshly built debug binary, `npm pack` all five, install the tarballs into an empty directory on ubuntu and macOS, and assert `minuta --version` equals the `Cargo.toml` version. This is the check that would have caught a wrong `files`, a missing mode bit or an unpinned optional dependency.
- Release workflow: a `workflow_dispatch` dry-run input that runs everything except `npm publish` (uses `npm publish --dry-run`), so the pipeline can be tested before the first real tag.
- `make ci` stays Rust-only; the Node tests run in their own CI job so Rust contributors do not need Node.

## Decisions
- New: [ADR-0014](../adr/0014-npm-prebuilt-binaries.md): npm distribution as prebuilt binaries in per-platform packages.
- CONTEXT.md: no new terms needed.

## FAQ
### Why not WASM?
minuta's value is orchestrating external processes: it starts `ffmpeg` and `whisper-cli`, kills process groups, handles SIGTERM, and caches to disk. A WASM build cannot spawn processes or send signals, and the tools it drives would still need to be installed natively. The result would be a different, much smaller program.

### Why not a `postinstall` that downloads the binary?
It fails with `--ignore-scripts`, behind proxies and in offline mirrors, and it makes installs depend on GitHub at install time. Platform packages come through the registry the user already trusts and caches.

### Why `@jcioriente/minuta` and not `minuta`?
The org is owned and the scope cannot be squatted. `npm view minuta` also returned 404 on 2026-11-03, so the unscoped name looks free; claiming it later can be a follow-up that re-publishes the launcher under that name.

### Why musl for Linux?
One static binary per architecture works on Ubuntu, Debian, Alpine and WSL2 images without tracking glibc versions.

## Open questions
1. **Publish credential.** Recommended: `NPM_TOKEN` (automation token scoped to the `jcioriente` org) in repository secrets for the first release, then switch to npm trusted publishing (OIDC) once the packages exist, since trusted publishing is configured per existing package. Needs the maintainer to create the token and the secret.
2. **musl feasibility. Resolved: it builds.** `cargo build --release --locked` on Alpine (`rust:alpine`, Rust 1.99.1, `--platform linux/arm64` and `linux/amd64`) built the full tree for `aarch64-unknown-linux-musl` (1m17s) and `x86_64-unknown-linux-musl` (1m52s), both statically linked, and each binary printed `minuta 0.8.0`. Not covered by that check: cross-compiling from a glibc runner (ubuntu-latest). The build task uses `musl-tools` for x86_64 and a cross C compiler for aarch64 (for `ring`), and fails the job if `file` does not report static linking. The glibc fallback is not needed.
3. **Release trigger today.** Releases are made by hand (tags `v0.6.0`..`v0.8.0` and GitHub releases exist; no release workflow). Confirm that pushing the tag is the moment to publish, or whether a GitHub Release `published` event is preferred.

## Implementation status
Updated by the PRs that implement this spec.
- [ ] Plan and issues
