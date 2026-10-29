# Public backend contracts

Run a focused public contract with `rtk cargo test --locked --test SUITE`, replacing `SUITE` with `durable_chat_http`, `recovery_http`, `memory_http`, `context_http`, `projects_http`, `api_efficiency_http`, `realtime_http`, or `push_http`. Docker is required: each suite starts a disposable PostgreSQL 18/pgvector instance using the pinned image digest and production migration chain. Failures are not skipped. External paid AI and push HTTP boundaries use local protocol simulators; no customer accounts or provider charges are used.

The suites cover native bearer sessions, ownership, bounded pagination and insertion snapshots, immutable edits/regeneration, durable streaming/replay, images and editing, cancellation/deletion, shared attachment cleanup, and restart/unknown-outcome recovery. Memory contracts cover per-owner versions, idempotency, bounded keyset pages, UTF-8 byte limits, disable preservation and running-generation cancellation. Context contracts cover selected ancestry, memory corrections, summary/extraction fencing and keyword/vector fallback. Project contracts cover scoped idempotent CRUD, assignment/detachment and version changes. API-efficiency contracts cover chat-create replay, deleted replay and byte-bounded pages without record loss. Realtime contracts cover private/coalesced replay, resync, retention, cursor validation and session revocation; its negative-cursor property uses 8 cases with seed 73641988. Push contracts cover owner/session-bound devices, token privacy, preview opt-in, default minimal alerts and local FCM/APNs delivery simulation. Cursor/limit properties run 24 cases with seed 73641981. Regression tests for disappearing branch pages and unreclaimable images were observed failing before the fixes.

Tests consume public HTTP and the deliberate Application assembly API. SQL in the recovery test only injects process-loss state; resulting behavior is asserted over HTTP. Native Flutter and live-provider acceptance are separate pending checks. See the [testing policy](../docs/testing.md) and [verification record](../docs/verification/2026-10-29-chat-backend.md).

The realtime retention journey uses a bounded database fixture to cross the 1,000-event retention boundary, then asserts resynchronization over public SSE. It does not assert private trigger call order. Testcontainers execution requires Docker access; a compile-only check is not a PostgreSQL behavior pass.

The push simulator verifies application HTTP/authentication/payload handling, not live FCM/APNs delivery, native notification rendering, device action handling, or extension behavior. A passing push HTTP contract is not device acceptance.

For the new memory/context, projects, API-efficiency, realtime and push section,
see the [2026-10-29 verification record](../docs/verification/2026-10-29-memory-backend.md).
Its compiled scenarios have not passed PostgreSQL runtime acceptance in the current
restricted session; earlier chat/profile suite results do not cover this section.

`rtk cargo test --locked --test health_http` checks unauthenticated readiness against real PostgreSQL, keeps chat routes authenticated, then closes the shared runtime pool and verifies a bounded 503. Readiness needs no schema; this operational scenario intentionally does not run migrations.
