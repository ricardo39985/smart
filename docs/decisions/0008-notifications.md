# 0008: Native push and one-way resource invalidation

Date: 2026-10-29

Status: Accepted for the user's appended notification and live-update work.
Written implementation is not live-provider or native-device acceptance.

## Decision

Reuse SSE for one-way invalidations instead of adding a WebSocket protocol and
dependency solely for server-to-client hints. `/v1/events` carries resource IDs and
actions, never transcripts. The existing per-generation stream remains responsible
for token delivery. Native clients should coalesce events, invalidate cached state
and fetch only affected visible resources; they must not fetch the whole account
or every conversation after every event. Reconnect supports owner-scoped durable
sequence replay and a resync instruction when retention has expired.

PostgreSQL transactional triggers record changes with a per-owner clock, avoiding
the commit-order gaps of a global identity sequence. Keep at most 1000 recent
events per owner, with a seven-day replay window. One SQL snapshot combines
revocation checks, retention bounds and the next batch. Generation and invalidation
streams share a bounded admission pool. See [realtime operations](../operations/realtime.md).

Android uses FCM HTTP v1 with a cached service-account access token. iOS uses APNs
HTTP/2 with a cached ES256 provider token. Both are absent/disabled by default;
configured credentials are protected runtime files, never client input. Device
registration is authenticated, bounded, session-bound and never returns tokens.
Successful generation commits enqueue completion notifications transactionally.
Delivery has bounded concurrency, attempts, TTL, coalescing and invalid-token handling.
At-least-once transport can still duplicate a notification after an unknown delivery
outcome; collapse identifiers reduce duplicates but are not exactly-once delivery.

Rich notifications carry deep links, platform categories/channel metadata, grouping
and meaningful action IDs. Generic previews are default. Opt-in text previews are
bounded; image previews use private attachment references, never public image URLs
or bearer tokens. Native clients must supply Android rendering/action handlers and
iOS categories, app-group credential access and a notification service extension.
No critical-alert or time-sensitive entitlement is implied. See
[push operations](../operations/push.md).

## Verification and consequences

Public HTTP journeys use production PostgreSQL migrations and local FCM/APNs
protocol simulators. Real platform credentials, native permissions, background
delivery, lock-screen previews and action handling require device acceptance.
The current sandbox blocks Docker and Danny dispatcher access; those gates are
reported as blocked rather than passed. No production deployment is authorized by
this decision. Native client integration is separate from this backend repository.
