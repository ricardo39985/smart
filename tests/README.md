# Public backend contracts

Run `rtk cargo test --locked --test durable_chat_http` and `rtk cargo test --locked --test recovery_http`. Docker is required: each scenario starts a disposable PostgreSQL 18/pgvector instance using the pinned image digest and production migration chain. Failures are not skipped. Only paid provider HTTP protocols are simulated. No customer accounts or provider charges are used.

The suites cover native bearer sessions, ownership, bounded pagination and insertion snapshots, immutable edits/regeneration, durable streaming/replay, images and editing, cancellation/deletion, shared attachment cleanup, and restart/unknown-outcome recovery. Cursor/limit properties run 24 cases with seed 73641981. Regression tests for disappearing branch pages and unreclaimable images were observed failing before the fixes.

Tests consume public HTTP and the deliberate Application assembly API. SQL in the recovery test only injects process-loss state; resulting behavior is asserted over HTTP. Native Flutter and live-provider acceptance are separate pending checks. See the [testing policy](../docs/testing.md) and [verification record](../docs/verification/2026-10-29-chat-backend.md).

`rtk cargo test --locked --test health_http` checks unauthenticated readiness against real PostgreSQL, keeps chat routes authenticated, then closes the shared runtime pool and verifies a bounded 503. Readiness needs no schema; this operational scenario intentionally does not run migrations.
