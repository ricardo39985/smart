# Memory, organization and notification backend verification

Date: 2026-10-29. Base commit: `1a692be8158cc9167a0fc545ca1a734ef678b68c`.
This record covers an uncommitted shared working tree, not a deployed revision.
The [source manifest](2026-10-29-memory-backend-source.json) records 50 source,
test, migration and contract files. Its aggregate SHA-256 is
`54281260d04e4272cfe02eee73e0b443bbf33f85b671ed4528f592442ca3055e`.
Strict all-target Clippy and focused push compilation passed after the final
notification race fixes; the broader compile and documentation checks preceded
those narrow fixes.

## Delivered source

- Stable Cindy instructions, bounded branch-aware recent context, local hybrid
  recall and source-attributed background summaries/memories. Saved memories have
  ownership, versions, correction/deletion/disable controls and stale-job fences.
- Pinned local Bekko/OpenVINO embedding service and offline evaluation command.
  Keyword fallback is available when local inference is unavailable.
- Bounded extraction/summarization batches, stable prompt prefixes, cached and
  uncached usage counters when supplied by the provider. No cost or latency
  improvement is claimed from compilation or source inspection alone.
- Projects, chat assignment/detachment and existing chat lifecycle support.
  Optional chat-creation request IDs and bounded whole-record history pages
  prevent duplicate creation and oversized ordinary history responses.
- One-way SSE invalidation hints with durable replay, coalescing and resync.
  Android FCM/iOS APNs registration, bounded durable completion delivery and rich
  private-preview/action metadata. Native rendering/actions remain client work.
- All-route inbound/outbound [API audit](2026-10-29-api-audit.md), including the
  existing Flutter consumers and explicit remaining optimization opportunities.

The concurrent UI task owns profile source, profile migration `0003`, profile
tests, and the coherent disabled email/password boundary. This task neither stages
those hunks nor deploys a mixed artifact. Its isolated profile verification and
deployment evidence must not be substituted for verification of this section.

## Checks and outcomes

Compiler: Rust/Cargo 1.96.0. Final Cargo checks use the pinned lockfile. Remaining
builds are serialized with `CARGO_BUILD_JOBS=1` following shared-machine resource
coordination; no native Android build or Docker suite is launched here.

| Result | Check | Meaning |
| --- | --- | --- |
| Passed | `cargo fmt --all -- --check` | Rust formatting only. |
| Passed | `rtk cargo clippy --locked --offline --all-targets -- -D warnings` | All-target strict lint/type checking. |
| Passed before final push race fixes | `rtk cargo test --locked --offline --all-targets --no-run` | All test targets compile; no behavioral test executed. Final push changes receive strict all-target Clippy and focused compilation below. |
| Passed after final push race fixes | `CARGO_BUILD_JOBS=1 rtk cargo test --locked --offline --test push_http --no-run` | Final notification code and new concurrent-removal/recovery regressions compile; no runtime pass claimed. |
| Passed, zero doctests | `rtk cargo test --locked --offline --doc` | Documentation test target completes; this is not product test evidence. |
| Passed | `RUSTDOCFLAGS=-Dwarnings cargo doc --locked --offline --no-deps` | Rust documentation builds without warnings. |
| Passed | Python AST parse of `embedder/*.py` | Syntax only; no embedding model loaded. |
| Passed | JSON parsing and resolution of every local OpenAPI reference | 33 paths, 48 method/path contracts, 317 local references; not a formal OpenAPI validator pass. |
| Passed | Unique migration prefixes and `git diff --check` | Nine migration names and whitespace checks; no claim that PostgreSQL applied them. |
| Blocked | `sh scripts/check` | Formatting/lint completed; its first database scenario failed at Testcontainers startup: Docker socket `Operation not permitted`. The script did not complete successfully. |
| Blocked | Focused database regressions, properties and mutations | Require real PostgreSQL/pgvector and a passing baseline; no SQLite substitute, skipped tests or killed-mutant claim. |
| Blocked | Trusted Danny dispatcher preflight | Connection failed with `socket: Operation not permitted`; no job was submitted. The documented runner also lacks executable test workspace/Docker access. |
| Blocked | Real embedding download/evaluation | Dependencies/model downloads unavailable in this environment; syntax and client compile are not quality or latency measurements. |
| Not run | Live DeepSeek/BFL/FCM/APNs compatibility | No protected provider credentials or paid traffic used for this section. |
| Not applicable here | Native Android/iOS acceptance, deployment | Separate client/operator work; no native notification rendering or production readiness claim. |

An earlier push adapter compile error (`Vec<Value>::to_string`) was fixed; the
unchanged affected test targets subsequently compiled. No unresolved compiler
failure is being reclassified as a passing behavioral test. The Blaze iteration
attempt failed because its runtime directory belonged to another user; pinned
Cargo remained the authority.

## Regression and property evidence limits

New public scenarios cover memory ownership/lifecycle, topic changes and return,
branch selection, stale extraction/corrections, provider usage and cancellation;
project lifecycle and pagination; chat retry and response-byte limits; SSE replay
and revocation; device privacy/session lifetime and notification protocols.

The identical device-registration and expired-session token-recovery regressions
were added before fixes, but Docker denial prevented observing their intended red
failure or green behavior. The same applies to the final device-removal/enqueue
race regression. Source review found that a recipient removed between selection
and outbox insertion could roll back a completed paid reply; the fix holds device
key-share locks through enqueue. This is reviewed code, not observed race coverage.
Review also found that concurrent expired-lease maintenance could overwrite a
fresh delivery lease selected through an older statement snapshot. Bounded
outbox-only `FOR UPDATE SKIP LOCKED` candidate selection now prevents that stale
update. Its recovery regression was added before the fix and also remains runtime
unverified. No new source-review finding is called an observed red/green result.
Final contract review also found that notification links used the production app
scheme even when authentication selected the development app. Assertions for the
configured development scheme were added before the fix. The Docker restriction
prevents observing this regression at runtime; the link now uses the already
validated application scheme, defaulting to `smartcindy` when none is configured.

Seeded public properties are retained for memory limits/versions (8 cases,
`73641983`), projects (8, `73641986`), SSE cursor rejection (8, `73641988`), chat
pagination (4, `73641989`), and device normalization/ownership (8, `73591245`).
They compile but have not passed in this session. Focused mutations for ownership,
deletion/epoch fencing, cursor ordering and device/lease rules remain required.

## Remaining acceptance

Run the affected public suites and focused mutations on a reviewed Docker-enabled
host, with serial suite execution while the machine is shared. Apply all production
migrations to the disposable PostgreSQL/pgvector database. Exercise concurrent
correction/deletion, worker takeover and device/session removal, then run the
pinned embedding model's topic/paraphrase/latency evaluation. Live provider smoke
checks and Android/iOS notification permissions, reply/copy/open actions, secure
image loading and background delivery require their own evidence.

See [current status](../status.md), [working plan](../memory-implementation-plan.md),
[memory decision](../decisions/0007-memory-context.md) and
[notification decision](../decisions/0008-notifications.md). Earlier deployed
chat/profile evidence does not establish acceptance of this new source snapshot.
