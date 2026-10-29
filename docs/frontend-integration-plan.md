# Native frontend integration plan

Current status (2026-10-29): the integration is activated and the development
API/PostgreSQL have moved to Inspiron; Danny remains the build workhorse. See
[local runtime operations](operations/local-development-api.md) and
[move evidence](verification/2026-10-29-local-runtime-move.md). The Flutter client
integration, native push source and subsequent scrolling fixes are committed
through `8d27bea` (APK 0.3.4+7). Its own status and verification records govern
client acceptance. The preparation/patch instructions below are historical;
do not reapply the original patch to the current client checkout.

Date: 2026-10-29. Status: core integration source implemented; runtime acceptance and native push pending. See [source handoff](verification/2026-10-29-native-integration-source.md) for delivered scope and actual verification.

The sequence below preserves the original acceptance plan; it is not a claim that every gate has passed.

This review inspected backend commit `4c61927` and Flutter commit `babd6ad`, with
both working trees clean before this document was added. It compared routes and
schemas with the actual client consumers. No runtime, provider, database or device
checks were run. [Status](status.md) remains the implementation/evidence authority;
[API](api.md), [OpenAPI](openapi.json) and accepted ADRs remain the contracts.

## Findings

| Capability | Backend source | Flutter today | Integration work |
| --- | --- | --- | --- |
| Accounts/profile | Sessions, signup, identity linking, private avatars | Connected; local native account/profile evidence recorded | Reuse session lifecycle; retain pending live Google/Apple and iOS acceptance |
| Chat | Durable requests, workers, cancellation, SSE, history | Local timer/fixture responses | Real request lifecycle, persisted retry identity, streaming and recovery |
| History/branches | Paged chats/messages/branches, immutable edits and regeneration | Sample conversations; process-local turns | Stable IDs, anchored pagination, branch selection and lifecycle controls |
| Photos/Create | Private upload/download, vision, image generation/editing | Attachment placeholder and sample image/timer | Native selection, bounded upload, authenticated display and real generation |
| Projects | Versioned CRUD and chat assignment | Sample project cards | Real projects, filtered chat pages and conflict handling |
| Personal memory | Versioned controls, source references and server context workers | No consumer | Saved-memory management and enabled setting |
| Live changes/push | Owner invalidation SSE; session-bound FCM/APNs foundation | No consumer | Foreground reconciliation, permissions, native delivery/actions |
| Research/billing/recovery | Research/search tools, billing, verified email/recovery and account erasure remain pending | Unavailable controls or no flow | Separate backend/product work; not unlocked by wiring |

The newer backend section is recorded as implemented but not behaviorally verified
or deployed. The handoff identifies the development runtime as the earlier profile
build. This review did not independently recheck its running revision or health.

## Resolve before the affected client slice

1. **Backend readiness.** Run the new real-PostgreSQL contracts, address failures,
   and prepare a reviewed development build/migration before accepting native flows
   against it. Earlier chat/profile evidence does not validate the newer section.
   Docker access was blocked in the recorded session; check current availability
   on an approved development host rather than assuming that historical failure
   persists. Do not change the Danny worker isolation to run Testcontainers.
2. **Generation discovery.** Chat metadata has no active/latest generation, and
   user-message `generation_id` is deliberately null. Status/SSE require a known
   generation UUID. A local durable request record supports recovery on the sending
   device, but a fresh installation/second device cannot discover unfinished or
   failed attempts from history. Propose a bounded owner/chat/head-scoped lookup
   returning the relevant latest attempt, including terminal failure/cancellation.
   Specify its branch/regeneration semantics, document the public extension in an
   ADR/API/schema, and verify it before generating client types. Do not guess IDs
   or continuously poll every conversation.
3. **Response presentation.** The backend returns text/image parts, not the local
   ten-block `LocalResponse` fixture. Keep the existing text/image wire contract.
   Initially render text faithfully, with safe supported Markdown mapped to existing
   themed views and plain-text fallback for incomplete/unsupported syntax. Validate
   link schemes; do not load remote Markdown images automatically. Code is display
   content. Do not require a model to produce UI-block JSON. A richer semantic
   response protocol would be a separate reviewed contract change.
4. **Create geometry.** The provider adapter currently requests 1024×1024 and the
   public turn contract accepts no aspect ratio. Ship an explicitly square real
   result initially and make other formats unavailable with an explanation.
   Restoring functional Portrait/Landscape requires a reviewed backend extension
   and current provider verification; changing only the UI preview is insufficient.
5. **Library promises.** Chat pages are ordered by creation, not recent activity.
   No server conversation-search endpoint, excerpts, project counts or project
   artwork are provided. Replace fabricated rows/counts with real metadata and
   neutral placeholders. Label local filtering as filtering loaded titles; do not
   imply complete history/content search or download all history to simulate it.
   Full search and a true activity-sorted list require explicit backend work if
   retained as product requirements.
6. **Client generation.** OpenAPI supplies JSON schemas, while SSE is described as
   strings with behavioral payloads in the API guide. Validate generated Dart support
   for OpenAPI 3.1 unions, nulls, integers and discriminator constants on a small
   representative slice first. Add stable operation IDs if the chosen generator
   needs them. Generate JSON types from a pinned backend contract snapshot checked
   into the independent client repo; keep the small SSE adapter explicit and tested.

## Implementation sequence and acceptance

### 0. Establish the integration baseline

- Review current backend/client evidence and freeze a compatible contract snapshot.
- Run affected backend suites for memory/context, projects, API efficiency, realtime,
  push and reasoning-policy behavior against production migrations in disposable
  PostgreSQL/pgvector. Resolve substantive failures before deploying those routes.
- Complete the generation-discovery contract above. Establish a repeatable native
  journey fixture using the real Rust application/database/workers with simulated
  paid-provider boundaries. Keep test-only configuration out of delivered builds.
- Coordinate reviewed development migration/build activation through the documented
  operator workflow. Keep the Cloudflare connector running; diagnose origin/relay
  failures separately. No production changes belong to this integration plan.

Acceptance: the tested revision and deployed development revision are identified;
the app can authenticate and discover configured capabilities. Capability booleans
are availability hints, not proof that a paid provider works.

### 1. Deliver one durable text conversation

- Keep `app.dart` focused on navigation. Add concrete chat API/state modules and a
  bounded authenticated transport shared with account operations where useful.
  Preserve secure storage and existing account checks; use a narrow authenticated
  request boundary rather than handing tokens to widgets.
- Generate only the JSON consumer types needed by this slice. Existing account
  transport limits of 64 KiB and a 15-second whole-request timeout are unsuitable
  for history and long-lived SSE. Use route-specific byte bounds, connect deadlines,
  heartbeat/idle detection, explicit cancellation and capped reconnect backoff.
  Disable redirects for authenticated requests; tokens never enter URLs.
- Create a real chat on first send, with a persisted creation request UUID. Persist
  each immutable turn request UUID/payload before transmission, then save the
  accepted chat/message/generation IDs. Retry an uncertain submission with exactly
  the same identity and payload; never mint another paid request automatically.
- Introduce a minimal durable pending-operation journal in app-private storage,
  scoped to environment and owner, excluded from backup, and purged on logout or
  identity change. Keep it bounded and expire resolved records. Select its concrete
  protection/storage mechanism before implementation; no offline transcript database
  is required for this first slice. PostgreSQL remains authoritative.
- Parse generation SSE across arbitrary UTF-8/network fragments; deduplicate event
  IDs and accumulate text incrementally. Persist cursor and partial state together,
  or replay from zero after process loss. A saved cursor without its text baseline
  must never skip part of the answer.
- On disconnect/resume, recover the accepted generation and replay. Use status when
  reconciling an interrupted/terminal stream; avoid parallel status polling while
  SSE is healthy. Partial failed/cancelled output must remain visibly incomplete.
- Stop invokes server cancellation and respects a completion race. Leaving a chat,
  selecting New chat or switching tabs only detaches the view; it does not cancel
  server work implicitly. New chat opens an empty draft and creates no empty server
  record until send.
- Replace fixture replies for signed-in users. Retain labeled examples only in the
  explicit Explore/preview path. Preserve current theme, composer and motion.

Acceptance: sign in, send, see streamed text, stop, reopen the saved chat, lose the
connection, and kill/relaunch the app without duplicate turns or provider submissions.
Fresh-device recovery discovers an existing attempt. An outage retains the session
and draft; a confirmed 401 clears private state and all owned streams. Late responses
from a prior identity cannot populate the next account's screen.

### 2. Connect Your space, history and branches

- Replace sample chat rows with lazy authenticated pages, honest empty/loading/error
  states, and create/open/rename/archive/delete controls.
- Prepend older whole-record message pages while preserving the visible scroll anchor.
  Keep independent cursors/cache windows per selected branch. Follow `next_cursor`,
  not assumptions that page length always equals the requested limit.
- Support immutable user-message edits, assistant regeneration and branch selection.
  Continue the explicit selected assistant parent; preserve original branches.
- Retain versions for mutations. On 409, refetch the affected resource, preserve the
  user's draft and explain/reconcile the conflict; do not blindly repeat destructive
  changes. Bound transcript/image memory and clear private state on identity change.

Acceptance: reopen durable history after restart; scroll backward without jumps,
duplicates or omissions; edit/regenerate and revisit both branches; exercise stale
versions and deletion during generation. Search describes its actual loaded scope.

### 3. Connect photos and Create

- Reuse native picker experience with chat-specific recovery/ownership state. Validate
  supported input locally for feedback; the backend remains the authority for MIME,
  dimensions, quotas and attachment ownership. Enforce at most four photos/turn.
- Upload raw bytes once and submit returned attachment IDs. Never resend complete
  transcripts or send gallery paths/provider URLs. Handle uncertain upload outcomes
  explicitly: attachment upload itself has no idempotency key in the current contract.
  Do not silently loop retries that consume the 64 MiB owner quota. Clean up known
  abandoned, unreferenced uploads through the real deletion route.
- Fetch private image bytes only from the configured authenticated origin. Bound
  decode/cache work and evict deleted/account-switched images. No thumbnail endpoint
  exists, so defer offscreen full-size downloads rather than assuming tiny previews.
- Use the same durable generation lifecycle for Create (`output:image`), show the
  actual result, and retain it in its real conversation. Editing explicitly attaches
  the chosen source image; earlier images are not implicit editing inputs.
- Gate text, photo questions, image generation and editing with their corresponding
  capability combinations and handle server rejections honestly. Keep model IDs,
  reasoning policy and all provider credentials on the backend.

Acceptance: ask about a picked photo, generate a square image, explicitly edit it,
reopen the result, and exercise cancellation, missing capability, oversize input,
quota exhaustion, revoked sessions and deletion. Simulated-provider acceptance and
capped live text/vision/image checks are reported separately.

### 4. Connect projects, memory and foreground changes

- Replace sample projects with real create/rename/delete/assignment flows and paged
  project-filtered chats. Deleting a project preserves its conversations. A project
  is organization, not a separate privacy or memory boundary.
- Add saved-memory list/create/correct/delete and an enabled setting in account
  settings. Preserve source references and handle row versions separately from the
  owner-wide settings epoch. Disabling preserves entries. Explain `memory_changed`
  cancellations and let the user choose whether to send again.
- Open one owner `/v1/events` stream while foregrounded. Coalesce invalidations,
  update/remove visible records, mark offscreen resources stale, and refresh visible
  lists on `resync`. Do not rebuild every tab or fetch every chat/account per event.
  Preserve anchored paging while refreshing membership; never splice incompatible
  snapshot cursors together.
- Coordinate lifecycle/resume with the existing account refresh to avoid duplicate
  profile/avatar reads. Adopt safe audited savings such as consuming the account
  already returned by a successful linking exchange, retaining identity checks.

Acceptance: move chats, delete a project without losing content, correct/delete/
disable memory and observe subsequent context behavior, and reconcile a second
device's changes without whole-account reloads. Local real-model recall/latency
evaluation remains a separate gate from keyword/simulated-provider journeys.

### 5. Implement native notifications and complete device acceptance

- Integrate Android FCM and iOS APNs registration, permissions, token rotation,
  preference changes and session-bound cleanup. Default previews off. Configure
  appropriate platform files/entitlements through reviewed private configuration.
- Route authenticated chat/generation deep links from cold/warm starts. Verify
  ownership through the API before showing content. Deduplicate completion alerts
  and reconcile foreground streaming with push delivery.
- Implement Android channels/actions/RemoteInput and iOS categories/service extension
  and appropriate app-group credential access for opted-in private rich previews.
  Background/locked-device actions must degrade safely when credentials are locked;
  explicit reply actions use durable request IDs and the correct resolved parent.
- Test real background delivery, open/reply/copy actions, previews, denied permission,
  revoked sessions, account switches and malformed/stale links on target devices.

Acceptance: notification descriptors become working native behavior. Real FCM/APNs
device delivery is verified separately from backend protocol simulators. Android
emulator evidence does not establish iOS or physical-device delivery.

## Verification and handoff

Use focused public contracts and native journeys during each slice. Bug fixes require
an observed behavioral failure before the unchanged regression passes. Apply bounded
property/mutation checks to substantive idempotency, ordering, ownership, deletion
and fencing changes per [testing policy](testing.md), selecting Dart tooling when
the relevant rules arrive rather than adding it to the preview shell.

At a coherent handoff, run the appropriate repository-local checks using the pinned
toolchains: backend `sh scripts/check` when backend code changes and client
`sh scripts/check` plus affected Android/iOS integration journeys for client work.
Use optimized release builds for delivery through the existing build scripts.
Keep screenshots/logs synthetic and content-free where possible. Record passed,
failed, blocked and not-applicable separately, with the tested revision and provider
boundary. Do not run application suites solely for this planning document.

First implementation milestone: slices 0–1, producing a real durable streaming chat
with restart/reconnect/cancellation acceptance. Subsequent slices complete the
existing core primitives without mixing in research, subscriptions or reminders.
Live paid-provider checks need protected test credentials and a stated capped budget;
macOS/Xcode and suitable devices are needed for iOS acceptance. Production abuse
controls, billing/distribution policy, account erasure, retention/reconciliation,
backup/restore and capacity measurements remain release work in status.md.
