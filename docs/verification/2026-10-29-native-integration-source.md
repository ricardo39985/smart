# Native integration source handoff

Date: 2026-10-29. Backend base: `4c61927`; frontend base: `babd6ad`.
Both are local working-tree changes, not a committed/released artifact.

Follow-up: the backend development binary has now built successfully and is
packaged with checksums. Remote activation remains blocked by the available
access; see [operator update](../operations/native-integration-update.md).
The [frontend continuation handoff](../frontend-chat-handoff.md) is ready for the
other chat. These follow-up results do not change the earlier check scope below.

## Delivered

Backend: selected-head generation discovery (including terminal attempts), literal
case-insensitive title search with cursor binding, OpenAPI/ADR updates and migration
0010 for generation lookup and pg_trgm title indexing. The new public HTTP scenarios
cover ownership, head selection, regeneration, literal search and cursor scope.

Flutter: authenticated normal-entry live Chat/Your space/Create, durable immutable
request recovery, SSE streaming/cancellation, selected branches and paged history,
image uploads/questions/generation/editing, responsive private images, actual project
management, memory controls and one coalesced foreground invalidation feed. Pending
requests are scoped to environment/owner/session; identity changes dispose transports
and private caches. A pinned schema generates response validation. No new runtime
packages, hosted CI, model controls or embedded provider credentials were added.

The writable frontend checkout is `.work/frontend-integration` under the backend
repository. The sibling `/home/rick/Documents/Code/smartcindy-app` is outside this
session's writable roots and was left unchanged. The complete frontend diff is
packaged as `.work/smartcindy-frontend-integration.patch`, applying to `babd6ad`.
The frontend's `docs/live-integration.md` documents its runtime and test boundaries.

## Checks

| Result | Check | Scope |
| --- | --- | --- |
| Passed | `rtk cargo check --locked --offline --all-targets` | Pinned Rust 1.96 compilation/type checks of backend and tests after the API/index additions |
| Passed | Rust formatting, OpenAPI JSON parse, backend diff whitespace | Source consistency only |
| Passed | Direct pinned Dart `--suppress-analytics analyze lib test integration_test` | Final complete frontend source and test analysis, no issues |
| Passed | Direct Dart formatting of changed source/tests | Source formatting |
| Passed | `DART=<pinned SDK executable> python3 scripts/generate-api-contract.py --check` | Deterministic generated response validator matches copied backend contract |
| Passed | `git apply --check` against the unchanged sibling frontend checkout | Complete patch applies cleanly to the current frontend base |
| Blocked before execution | Focused `flutter test --no-pub test/live_api_test.dart` | Flutter launcher could not update its engine stamp in the read-only installed SDK cache. No behavioral test ran; no SDK repair was attempted. |
| Not run in this phase | PostgreSQL/Testcontainers HTTP regressions and mutation/property execution | User requested implementation focus and bounded verification; prior infrastructure limitations remain documented separately |
| Not run in this phase | Native Android/iOS acceptance, screenshots, release builds, live paid providers, embedding evaluation | No runtime/device/provider claim follows from static analysis |
| Not applicable | Production deployment/store publication/hosted CI | None performed |

Focused Flutter protocol tests are retained for fragmented SSE/cancellation,
stale-session rejection, large history pages, immutable shared image caching,
terminal replay recovery, immutable pending journals and continuation after history
window eviction. A native journey requires explicit synthetic-account configuration;
missing configuration fails it rather than silently skipping acceptance. Test TLS
keys are documented public synthetic fixture data, never operational credentials.

## Remaining gates

- Apply/review this frontend patch in its normal repository; build on a writable
  pinned SDK when native checks are requested.
- Verify the migration chain, pg_trgm availability/privileges and affected real
  PostgreSQL contracts. Activate the reviewed backend build/migrations through the
  documented development operator workflow. The existing development runtime was
  not changed here. Preserve the continuously running Cloudflare connector.
- Native push registration, rich notification actions and notification deep links
  remain client work; notification settings stay honestly unavailable.
- Live provider compatibility, Google/Apple native flows, iOS/Xcode, and the existing
  billing/recovery/erasure/research/reminder/release gates remain explicit in status.

This is a connected source foundation, not a verified live mobile release.
