# Domain context

Shared vocabulary for people and agents working on this project. Use these terms exactly, in
code, specs and issues. When a term is ambiguous or keeps needing explanation, add it here.

| Term | Meaning | Not to be confused with |
|---|---|---|
| Source | One or more media files given to a run. Several are joined into one. | A job: the source is the input, the job is the folder of work |
| Transcript | Timestamped segments `{start, end, text}` in seconds, plus the detected speech language. minuta's own schema. | whisper.cpp's raw JSON, which is only parsed into it |
| Job | One run over one source: a folder `./minuta-out/<name>/` holding `job.json` and every artifact. | A process; a job persists across runs |
| Step | One stage of a job (`probe`, `join`, `audio`, `transcribe`, `render`) with declared inputs and outputs. | A template (sub-project #2) |
| Artifact | A file a step produces and stores in the job folder. | |
| Fingerprint | Hash of a step's inputs, parameters and tool version. A mismatch makes the step stale. | A checksum of the output |
| Stale | A step whose recorded fingerprint differs from the current one, or whose output is missing. | Failed |
| Speech language | The language whisper transcribes. `auto` or a code. | Output language and prompt language (sub-project #2) |

## Relationships
A job has one source and an ordered list of steps. Each step has at most one recorded
fingerprint and produces artifacts. A step is stale when its fingerprint or any earlier
step's fingerprint changed.

## Decisions
Links to ADRs that define domain behavior: docs/adr/
