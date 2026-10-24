# ADR-0002: Call whisper.cpp as a subprocess

- Status: accepted
- Date: 2026-10-23
- Deciders: Arnold Gandarillas Castillo
- Related: docs/specs/2026-10-23-core-pipeline.md

## Context
minuta needs whisper.cpp transcription with timestamps and VAD. It can embed the library
(`whisper-rs`) or run the installed `whisper-cli` and read its `-oj` JSON. The bash pipeline
already uses `whisper-cli`.

## Options considered
1. **`whisper-cli` subprocess**: no C++ build in `cargo install`; the user's Metal/CoreML
   build is reused; upgrades are independent. Needs a separate install and version checks.
2. **`whisper-rs` embedded**: one binary, but needs cmake and a C++ toolchain at build time
   and the GPU backend flags are fragile.

## Decision
Run `whisper-cli` as a subprocess and parse its JSON into minuta's own `Transcript` schema.

## Consequences
`minuta doctor` and preflight must check the binary, its version and the model. The parser
is isolated in one module, so a future switch to `whisper-rs` changes only that module.
Revisit if installing whisper.cpp proves to be the main adoption blocker.
