# Live resource invalidations

The current requirement is server-to-client notification that owned resources
changed. `GET /v1/events` uses authenticated SSE, reusing the existing streaming
transport without a second WebSocket protocol or dependency. HTTP commands remain
the write interface. Generation token/delta replay remains on its existing route.
This is a backend foundation, not evidence of native app integration or push delivery.

Connect with the native bearer Authorization header; credentials are never accepted
in URLs. On a first connection omit `after`. The `resync` event has an SSE event ID
and JSON `{seq,action:"resync"}`. Refresh current lists/account/settings, then retain
that cursor. Fetches should reconcile cached resources, preserving local drafts.

Reconnect with `?after=N` or `Last-Event-ID: N`; if both are supplied they must
agree. A valid cursor replays `changes` events with JSON
`{seq,changes:[{resource,id,action}]}`. Resource is `chat`, `project`, `memory`,
`memory_settings` or `account`; action is `invalidate` or `remove`. Payloads contain
no names, message text, memory content, tokens or profile details. Invalidations
cover chat creation/head/title/archive/project/deletion, project changes, memory
content/deletion, memory settings/epoch, account fields/identities and photos.
Generation completion changes the chat head; partial tokens are not duplicated.

Each batch reads at most 128 durable events and coalesces each resource to its latest
action. Clients fetch affected visible resources once per batch, invalidate other
cached lists, and remove deleted resources. Persist the batch cursor only after
applying all its invalidations. A failed fetch is still dirty and must be retried;
do not treat notifications as authoritative resource contents.

Only the newest 1000 events per owner are retained. Events older than seven days
are excluded from replay and pruned on the next change. A retention gap emits
`resync` at the current high watermark. A future/negative cursor returns 400.
The cursor is interpreted only within the authenticated owner; it grants no access.
Each poll combines session validity, replay boundary and rows in one SQL snapshot.
Session expiration/revocation closes the stream on the next poll; previously buffered
network bytes cannot be recalled. SQL faults close it rather than pretending success.

Idle streams poll once per second, with 15-second SSE comment heartbeats. Streams
share the existing 32-per-process admission limit with generation SSE. Database
connections are released before network yield/sleep. Multiple processes need an
operator-set total capacity limit; there is no unbounded application subscriber queue.

Database triggers create minimal invalidations in the same transaction as changes.
The per-owner transactional clock serializes event allocation and commit, preventing
late commits from appearing behind an already acknowledged cursor. Multi-resource
mutations lock the owning user before resource rows, then the trigger locks the clock.
This preserves owner-first ordering and avoids clock/resource deadlocks. The existing
leaf photo API is an explicit exception: photo then clock, followed only by unlocked
MVCC account reads. No current mutation holds a clock and subsequently writes a photo.
Before adding account erasure or a combined photo/account transaction, unify those
paths under owner then photo then clock ordering. No-op field updates and vector/index
writes do not emit notifications. Rollbacks roll back their clock increments/events.

The migration and public HTTP contract tests cover the intended behavior. Docker/
PostgreSQL execution and mobile reconnect/scroll-state acceptance must be reported
separately; code compilation alone does not establish either. Native background push
needs its separate APNs/FCM registration, credentials, delivery and device validation.
