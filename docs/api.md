# Native chat API, v1

Implementation contract for this backend slice; verification gates are in [status](status.md).
Product routes start `/v1`; the operational readiness route is `/healthz`. JSON uses UTF-8, UUID strings and UTC RFC3339 timestamps. Optional
response fields are explicit `null`. Unknown request fields, malformed UUIDs/cursors and
out-of-range limits are rejected. Errors: `{"error":{"code":"invalid_request","message":"invalid_request"}}`.
Codes/messages are deliberately generic, never raw SQL/provider responses.

Hosted model selection and reasoning are backend-owned. Turn/regeneration requests
accept no model or reasoning fields. The current default Flash adapter has internal
none/low/high/max reasoning controls; see [server operations](operations/deepseek.md).
No provider-specific native endpoint or model selector is added.

Status meanings: 400 invalid input/unsupported capability/context overflow; 401 absent,
expired or revoked bearer; 404 absent or not owned (indistinguishable); 409 version,
idempotency, reference or active-generation conflict; 410 replayed deleted project or chat; 413
bytes/quota exceeded; 429 login/admission/decode/project capacity; 503 provider or email
delivery not configured; 500 storage/internal fault.
Every response is `Cache-Control: no-store`. No wildcard CORS or trusted user-ID header.
JSON bodies are capped at 16 KiB by default, 8 KiB for account/auth flows, and 256 KiB for
turn submissions. Raw chat attachments are capped at 8 MiB and account-avatar uploads at
2 MiB; oversize bodies return 413. Content-specific limits may be lower.
Chat, projects, memories, devices, attachments, account profile, logout and capabilities require
`Authorization: Bearer TOKEN`.
Signup, login, auth methods, provider start/callback and proof exchange do not; linking start additionally requires a bearer session.
Every auth response is no-store. Native clients must use an HTTP streaming client that can set headers,
not browser EventSource assumptions. Never put bearer tokens in URLs.

## Operational readiness

`GET /healthz` requires no authentication. It returns 200 `{"status":"ok"}` when
PostgreSQL answers `SELECT 1`, or 503 `{"status":"unavailable"}` on failure or a one-second
timeout covering pool acquisition and query. Responses are JSON and no-store, with no
database details. This does not verify paid providers or workers. Root `/` returns 404.

## Sessions and capability discovery

- `POST /sessions {email,password}` -> 200 `{token,expires_at,user_id}`. Email is trimmed
  and lowercased; password is not normalized. Opaque random 256-bit token, 30-day expiry.
  PostgreSQL holds only SHA256(token), not the token. At most 16 live sessions/account;
  login evicts the oldest excess sessions. Login has two concurrent Argon2id jobs and
  a global 16 attempts/second/process bound (including nonexistent accounts). An edge
  per-IP/account abuse policy and multi-process rate-limit coordination remain launch gates.
- `DELETE /sessions/current` -> 204; subsequent use is 401.
- `POST /accounts {email,password,display_name}` -> 201, the same session shape. Password is 12..1024 bytes; normalized email must be at most 254 bytes with valid simple syntax; trimmed display name is 1..80 characters. Existing email -> 409 `account_exists` without revealing methods. No email verification or password recovery is offered.
- `GET /account` -> `{id,email,display_name,display_name_source,avatar_url,has_custom_avatar,capabilities,methods}`. Methods contains actual identities (password first, Google then Apple). `display_name_source` is `local`, `google`, or `apple`; `avatar_url` is a validated Google HTTPS picture URL or null (the server never fetches it); `has_custom_avatar` indicates an uploaded private photo. `capabilities` is `{email_change:false,password_change:false}`. Use the private avatar when present, otherwise the provider URL, otherwise initials. `PATCH /account {display_name}` trims and validates 1..80 characters without controls, sets the source to `local`, and returns the same shape. An existing local name is never overwritten by provider sign-in or linking. Email is the canonical account email and is locked in this slice; provider metadata refresh does not change it. Neither provider picture changes identity or local password login. A provider-only account has no usable password.
- `PUT /account/avatar` (authenticated binary PNG/JPEG/WebP, Content-Type matching decoded type exactly) -> 200 account shape. Body <=2 MiB (413 beyond); dimensions 1..4096 each and <=16 million pixels; malformed/mismatched data is 400. Bounded decode and downscale to <=512×512 preserves aspect and never enlarges; stored bytes are re-encoded PNG with metadata stripped. Replacement is atomic and separate from chat attachment quota. `GET /account/avatar` -> 200 `image/png`, no-store, nosniff, or 404 when absent. `DELETE /account/avatar` -> 204 (also when absent). All require a bearer (401 otherwise); no user-ID route parameter or public access.
- `POST /account/email-change` and `POST /account/password-change` currently return 503 `email_delivery_unavailable` without changing state. Their capability flags remain false until a verified email-delivery and recovery flow is implemented.
- `GET /auth/methods` (public) -> `{password:true,google:boolean,apple:boolean}` based on configured credentials; these are not provider-health indicators.
Server configuration: `AUTH_ORIGIN` is exactly `https://dev.smartcindy.app` or `https://smartcindy.app`, paired with `AUTH_APP_SCHEME=smartcindy-dev` or `smartcindy` respectively. Google requires both `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET`. Apple requires `APPLE_CLIENT_ID`, `APPLE_TEAM_ID`, `APPLE_KEY_ID` and an owner-protected `APPLE_PRIVATE_KEY_FILE` read only at startup. Partial configuration fails startup; neither the CLI nor the HTTP client can select provider endpoints. Configure different client IDs, callback registrations and signing material for development and production. Actual provider compatibility and native client acceptance are separate release gates.
- `POST /auth/{google|apple}/start {challenge,link?,confirm?,password?}` -> `{authorization_url,attempt_id}`. Challenge is the base64url/no-padding SHA-256 native PKCE challenge. A five-minute attempt holds a random hashed state and nonce; Google also uses a server-side PKCE verifier. Disabled provider -> 503 `provider_unavailable`. Link mode requires a valid bearer, `confirm:true`, and the existing account's password if it has one; provider-only linking requires a session created in the last five minutes. Missing confirmation -> 400; invalid credentials -> 401. The native client must separately explain and confirm that linking will add a sign-in method to the current account; matching email never authorizes linking.
- Google redirects to `GET /auth/google/callback?state=...&code=...` (or `error`); Apple submits `POST /auth/apple/callback` as `application/x-www-form-urlencoded`. Only an unclaimed matching five-minute state is accepted. After token redemption and ID-token signature/issuer/audience/expiry/nonce verification, the server responds 303 to the **fixed** dev `smartcindy-dev://auth/callback?code=...` or production `smartcindy://auth/callback?code=...` scheme. This code is a two-minute single-use completion proof, **not** a bearer session; no tokens appear in redirect URLs.
- `POST /auth/exchange {code,verifier}` verifies the original S256 challenge under a transaction lock and consumes the completion once. Wrong verifier does not consume it. Successful sign-in -> 200 session shape; successful linking -> 200 account shape (including avatar fields) plus `linked:true`, without changing the original session. Same-email signed-out collision -> 409 `account_link_required`; subject already linked elsewhere -> 409 `identity_already_linked`; cancelled provider -> 400 `provider_cancelled`. A provider-only account can reauthenticate by its provider subject even if Apple omits email on later grants. No identity is created before successful native proof exchange.
Google signed `name` and `picture` are optional. A valid trimmed name (1..80 characters, no controls) is used for a new provider account; otherwise it keeps the email-prefix fallback. A picture URL must be <=2048 bytes, HTTPS on googleusercontent.com or a subdomain, without userinfo, custom port, fragment or controls. Missing/invalid claims never clear previously verified profile metadata. Signed-out Google login refreshes its linked identity; an authenticated user may repeat the existing explicit link journey for the already-linked Google subject (client label: Refresh Google profile), with fresh confirmation, credentials/session check and native proof. This does not request offline access or store provider tokens. Email is never used to select or merge provider accounts. New provider account collisions include both reserved local emails and current verified identity emails; a refresh to an address already claimed by another user does not replace the previously displayed email. Email editing is not implemented.
- `GET /capabilities` -> `{text,image_input,image_output,image_editing,push,push_android,push_ios}`
  booleans from configured adapter credentials, not live provider health. Push platform flags
  indicate configured FCM/APNs credentials; no provider/model names or secrets are returned.
  Image input is available with either vision or editing; image editing follows image-output
  configuration.

Operator provisioning: `smartcindy provision-user EMAIL`, password from bounded stdin
or protected `SMARTCINDY_PROVISION_PASSWORD` environment, never an argument. Passwords
must be 12..1024 bytes. No recovery, email ownership verification, account deletion or billing is implemented.

## Chats and pagination

- `POST /chats {title,project_id?,request_id?}` -> 201 for a new chat, 200 for an identical
  live replay. `request_id` is optional to preserve older clients; when supplied, it is unique
  per owner and fingerprinted with the raw title and project ID. Repeating it with changed
  input returns 409; replaying a deleted chat returns 410. Title is at most 200 Unicode scalar
  values (empty permitted). An included project must be active and owned by the caller.
- `GET /chats?limit=30&cursor=...&archived=false&q=...` -> `{items,next_cursor}`. Limit 1..100.
  Sort is immutable creation sequence descending, **not updated_at**. A first-page
  owner-scoped high watermark excludes later inserts. Cursors are bounded (1024 chars),
  opaque base64url, typed/versioned and owner/filter scoped; wrong scope is 400. Optional
  `q` searches titles by case-insensitive literal substring (percent, underscore and exclamation
  mark are ordinary characters), after trimming and lowercasing. It is limited to 200 Unicode scalar
  values; a blank value behaves as no search. The normalized search is bound into the cursor,
  so changing or removing it during traversal returns 400. Search does not change creation
  ordering. A partial GIN trigram index supports substring matching; one- and two-character
  searches may still scan candidate chat rows because they contain too few trigrams.
  No total count/OFFSET. Renaming does not move rows. Archival/deletion changes membership;
  static traversal with concurrent inserts neither repeats nor skips preexisting items.
- `GET /chats/{id}` -> owned chat metadata, including current version and head, without scanning chat pages.
- `PATCH /chats/{id} {version,title?,archived?}` -> updated chat; at least one change required.
- `DELETE /chats/{id}?version=N` -> 204, soft delete and atomic cancellation of active work.
  Deleted resources return 404; stale workers cannot publish. Repeated delete is 404.

Every accepted turn/regeneration and successful publication increments chat version;
PATCH and DELETE compare the version currently returned by chat metadata/listing (stale -> 409).
Chat head changes only through explicit turn/regeneration admission and its fenced result.

Projects group chats by owner. Project IDs and cursors are owner-scoped; deleting a project
detaches chats and retains their content. See [Projects](#projects) for project lifecycle and
chat assignment routes.

## Branches, edits and immutable messages

- `GET /chats/{id}/messages?head_id=UUID&limit=50&cursor=...` ->
  `{items,head_id,next_cursor}`; limit 1..100, newest first. Omitted head selects the current
  head once. Empty chat returns null head, empty items. Cursor pins that selected head,
  even if new turns arrive. A supplied head must agree with cursor head. Wrong owner,
  chat, type or ancestry is rejected. Each page walks at most limit+1 ancestors via primary
  keys and targets 512 KiB of response bytes, including whole records only (at least one;
  one record larger than the target is returned whole). Cursor ancestry validation checks the bounded immutable ancestor-ID array, not
  a history scan. The backend does not fetch all history to serve a page.
- Items: `{id,parent_id,role,parts,depth,order,created_at,generation_id,status}`. Depth starts
  at zero, order is immutable creation sequence. Assistant messages identify their
  generation/status; user messages have null generation/status since regenerations can
  produce multiple jobs for one user node. Use acceptance/status APIs to track those jobs.
- `GET /chats/{id}/branches?limit=50&cursor=...` -> `{items,next_cursor}`; leaf heads
  (including unanswered user nodes), indexed creation keyset, snapshot high watermark.
  Pages target 512 KiB and include whole branch records only, returning at least one.
  A cursor lists heads as they existed at its high watermark. Continuing a branch later
  does not remove its previous head from an in-progress listing; refresh to see new heads.
  Reopening a branch means selecting its returned ID as messages head and explicitly
  continuing an assistant parent; no separate mutable branch-switch operation is needed.

`POST /chats/{id}/turns`:

```json
{"request_id":"UUID","parent_id":null,"parts":[{"type":"text","text":"Hello"}],"output":"text","replaces_message_id":null}
```

`parts` permits `{type:"text",text}` and `{type:"image",attachment_id}` only. At most 16
parts, 4 photos and 32 KiB combined text; a nonempty text or image is required. Parent is
null (new root) or an assistant in this chat, never an implicit latest parent. Editing sets
`replaces_message_id` to an existing user message and must match its original parent;
creates a new immutable user sibling, preserving original descendants. No in-place edits.

`POST /chats/{id}/messages/{user_message_id}/regenerations {request_id,output}` reuses that
user message and adds an assistant sibling; original assistants remain selectable.

Both -> 202 `{generation_id,user_message_id,status}`. User node + queued generation +
accepted event are one transaction. One queued/running generation/chat; concurrent
nonidentical contenders get 409. Global admission bound 128 active jobs; owner bound 8.
A request UUID is unique across **all endpoints/chats for an owner**. Canonical parsed
payload + endpoint fingerprint must match: exact retries return original identifiers and
current status, even after completion, restart or removal of provider configuration;
different payload with same ID -> 409. Deleted chats still return 404. No retry resubmits
paid work. The provider availability check follows existing-request lookup and precedes
new acceptance. Request object key ordering is immaterial; part/array order is material.

Text requests keep the selected branch's most recent 16..32 messages verbatim, bounded by
48,000 bytes of stored parts JSON. The immutable parent-linked ancestry can reach 4,096
messages; older relevant passages may be indexed and retrieved without replaying the full
transcript. A selected-branch summary is at most 8,000 UTF-8 bytes. Hybrid keyword/vector
retrieval can add up to eight saved memories or source-linked passages within a 6,000-byte
budget. Historical rows are included only when they belong to the selected ancestry. Vector
failure falls back to PostgreSQL keyword search; it does not call a hosted embedding API.
The generation status response reports bounded context metadata (`retrieval_mode`, recent
message count, recalled-memory/passage counts, summary use, estimated bytes, memory epoch,
and assembly time), not the private prompt text.

Text output begins with Cindy's stable server-owned personality instructions. Saved memories,
summaries and retrieved passages are explicitly untrusted historical data; current user
statements and corrections take precedence. Explicit user edits/deletions invalidate memory
epochs and running text generations. A reply already sent to a provider cannot be recalled or
its charge undone; stale results cannot be published after the change. Deleting saved memory
does not delete its source transcript. Deleting a chat removes its summaries, indexed chunks
and memories derived from that chat. Disabling memory preserves saved items and the visible
transcript, but suppresses cross-chat recall and extraction until re-enabled.

Text/image-input context admits at most 32 MiB of image bytes (repeated references count).
Image-output requests use the current user prompt and its images; historical photos are not
automatically image-editing references. Context overflow is rejected before acceptance; no
silent transcript truncation is presented as a complete answer.

## Personal memory and context jobs

- `GET /memory/settings` -> `{enabled,version}`. Memory is enabled by default at version 0.
- `PATCH /memory/settings {version,enabled}` -> the updated settings. `version` is an
  optimistic owner-wide memory epoch; stale versions return 409. Explicit control/content
  changes and background fact supersession can advance it; refresh settings before retrying a
  stale update. Disabling preserves user-saved entries, suppresses personal
  recall/extraction, advances the source watermark, cancels queued extraction and cancels
  currently running text generations with `memory_changed` so the next reply can use the new
  controls. Already streamed bytes and provider charges cannot be undone.
- `GET /memories?limit=30&cursor=...` -> `{items,next_cursor}`. Limit is 1..100. Items are
  active owned entries `{id,content,version,source_chat_id,source_message_id,created_at,updated_at}`;
  the first page uses an owner-scoped high watermark. At most 1,000 active entries are stored
  per owner. Entries remain visible in this management list while memory is disabled.
- `POST /memories {request_id,content}` -> 201 for a new entry, 200 for an identical replay.
  The request UUID is unique per owner and bound to a SHA-256 content fingerprint; a changed
  payload with the same UUID is 409. Content must be nonblank and at most 2,000 UTF-8 bytes.
  Creation is rejected with 409 while memory is disabled or the active-entry cap is reached.
- `PATCH /memories/{id} {version,content}` -> updated entry with its incremented row version.
  Stale versions return 409; another user's or deleted ID is 404. Content has the same
  nonblank 2,000-byte limit. Correction clears its old vector so it cannot be retrieved by
  stale semantic similarity.
- `DELETE /memories/{id}?version=N` -> 204. The stored content and embedding are cleared;
  only the tombstone ID and request ledger remain for idempotency. Stale version is 409;
  another user's or deleted ID is 404. Deleting a memory prevents extraction from the older
  transcript from restoring it by advancing the memory epoch/watermark.
- `GET /memory/jobs?limit=30&cursor=...` -> `{items,next_cursor}` with owner-scoped newest
  first keyset paging, limit 1..100. Items contain `{id,status,error,usage,created_at,chat_id}`;
  they expose costs/failures without message or memory content. Job statuses are queued,
  running, completed, failed and cancelled. Background work is bounded and never silently
  resubmits an unknown paid outcome. Usage counts are nonnegative when supplied; cached and
  uncached input counts are separate optional fields and omitted when unknown.

Every route requires a valid bearer. Settings and row versions are independent: settings
versions are the owner-wide epoch; each explicit user memory mutation's `version` is local
to that record. Automatic facts must quote an exact user-message excerpt and retain source
IDs. A manual correction is
never overwritten by extraction. Disabling/deleting memory controls future retrieval and
publication; it is not transcript erasure or a way to retract text already sent to a provider.
The worker indexes completed text locally in bounded batches and queues paid extraction and
summary refresh every eighth assistant reply. Global queued/running background work is capped
at 128, with at most eight per owner and one background provider call across processes. Each
extraction sends at most 48 KiB of source JSON, requests at most 2,500 output tokens, and has a
90-second HTTP timeout under a fenced 120-second lease. Replies do not wait for extraction;
new context is available from the recent transcript while background jobs catch up. A disabled
or stale job cannot publish after the owner epoch changes. Unknown paid outcomes fail closed
and are not automatically resubmitted.

## Projects

Projects group chats by owner. A project label does not change memory visibility or transcript
content; deleting a project detaches its chats and retains them. Project and chat mutations
use optimistic versions, and project IDs/cursors are always owner-scoped.

- `POST /projects {request_id,name}` -> 201 new project or 200 identical request replay.
  Name must be nonblank and at most 200 Unicode characters. Reusing a request UUID with a
  different raw name returns 409; replaying a soft-deleted project returns 410
  `project_deleted`. The owner may have at most 1,000 active projects; exceeding it returns
  429 `project_limit`.
- `GET /projects?limit=30&cursor=...` -> `{items,next_cursor}`; limit 1..100, immutable
  sequence order, owner-bound keyset cursor and first-page snapshot watermark.
- `GET /projects/{id}` -> owned active project.
- `PATCH /projects/{id} {version,name}` -> updated project and incremented version. Stale
  version -> 409; another owner's or deleted project -> 404.
- `DELETE /projects/{id}?version=N` -> 204 soft delete, detaching all chats and incrementing
  each affected chat version without deleting chat content. Stale version -> 409.
- `POST /chats/{id}/project {version,project_id}` assigns an owned active project, or detaches
  when `project_id` is explicitly `null`; both chat and project must belong to the caller.
  Successful assignment increments chat version even when the association is unchanged.
- `POST /chats {title,project_id?}` can create a chat directly in an owned active project;
  `GET /chats?...&project_id=UUID` filters chat pages by that project. Cursors bind the filter.

For `POST /chats/{id}/project`, `project_id` is required even when null. Project operations
and assignment require authentication and return generic errors that do not disclose another
owner's project existence.

## Durable generations and SSE

- `GET /generations/{id}` -> `{generation_id,user_message_id,status,result_message_id,error,usage,context,created_at,updated_at}`.
  States: queued, running, completed, failed, cancelled. Errors are sanitized codes.
  Usage is null when unknown, otherwise `{input_tokens,output_tokens,total_tokens}` plus
  optional `cached_input_tokens` and `uncached_input_tokens`. Supplied counts are
  nonnegative; unknown counts are omitted. Raw provider-specific usage fields are not exposed.
  `context` is null until assembly, then contains only bounded retrieval counts/mode and
  timing metadata, never prompt text.
- `GET /chats/{id}/generation?head_id=UUID` -> `{generation: Generation|null}`. An omitted
  `head_id` selects the chat's current head; an explicit ID must be a message in that owned,
  live chat. An assistant head returns that assistant message's generation. A user head returns
  its latest generation attempt, including failed/cancelled terminal attempts; this keeps an
  older branch anchored to its selected message. A head without an attempt (or an empty chat)
  returns `generation:null`. The lookup never resubmits provider work.
- `POST /generations/{id}/cancel` -> `{generation_id,status}`; idempotent for terminal states.
  A late cancel never overwrites a completed/failed result.
- `GET /generations/{id}/events?after=N` or header `Last-Event-ID: N` -> `text/event-stream`.
  Default after=0. If both are specified they must agree; negative/future IDs rejected.
  Event IDs are strictly increasing contiguous per-generation sequence numbers. Events:
  `accepted {generation_id,user_message_id}`, `delta {text}`, `image {id,url,mime,width,height}`,
  `completed {status,result_message_id,usage}`, `failed {status,error}`, `cancelled {status,error}`.
  Terminal replay ends the stream; replay after terminal yields an empty stream. Heartbeat
  comments every 10 seconds while waiting. Database errors terminate the stream; recover
  via GET status and replay rather than submitting a new request ID.

Events commit before emission. At most 32 live SSE response bodies per Application/process
are admitted; overflow is 429 stream_capacity, and disconnect/termination releases capacity.
Streaming polls at most 32 persisted events per batch,
releases database connections before yielding/waiting, rechecks session revocation/expiry
and chat ownership/deletion every batch (idle polling 200ms). It is not an atomic revocation
of already buffered TCP bytes. Disconnecting only detaches the subscriber, never cancels
or resubmits a job. Clients persist the last processed ID and deduplicate on reconnect.
Successful delta texts concatenate exactly to the saved assistant text. Partial deltas on
failed/cancelled jobs remain replayable but are not falsely promoted into a final answer.
Text output capped at 64 KiB; writes coalesce at 1 KiB or 250ms, bounded event budget 1020
text events. Usage is retained when supplied, including on failures.

Workers claim using SKIP LOCKED, UUID fence, 10s leases renewed every 250ms while provider
IO/context/decode runs, and a 120s deadline. Final answer/status/usage/image/events commit
atomically with lease and deleted-chat guards. Queued jobs survive restart; expired running
jobs become `failed/interrupted_unknown_provider_outcome`, **never automatically resubmit**.
BFL receipt is stored on acceptance when available. Operator reconciliation of unknown
paid outcomes remains required. Cancellation promptly drops local IO; remote processing
or charges may already have occurred. There is no HTTP submit retry or fallback.

## Realtime resource invalidations

- `GET /events?after=N` or `Last-Event-ID: N` -> `text/event-stream`, authenticated with a
  bearer header. Query and header cursors must agree when both are supplied. Cursors are
  nonnegative owner-wide event sequence numbers; a future cursor is 400.
- With no cursor or a cursor older than retained history, the server emits one `resync` event
  at the current high watermark: `id: N`, `data: {"seq":N,"action":"resync"}`. The client
  should refetch visible resource lists and resume after that ID.
- Otherwise the server replays bounded coalesced `changes` events with `id` and `data`:
  `{seq,changes:[{resource,id,action}]}`. Resources are `chat`, `project`, `memory`,
  `memory_settings`, or `account`; actions are `invalidate` or `remove`. Each event represents
  the latest action per resource in a batch. It contains no text, profile values or memory
  content; clients fetch only affected resources they already display.
- The owner event log retains at most 1,000 events for seven days. A response batches at most
  128 raw events and shares the 32 live SSE slots with generation streams. Session validity is
  rechecked every second and comments keep the connection alive every 15 seconds. Closing the
  stream only detaches the subscriber. This one-way invalidation feed does not replace the
  request-specific generation event stream. Generation completion invalidates the chat head;
  streamed deltas are never duplicated through this feed. Operational retention and lock-order
  details, including the leaf-photo exception, are in [realtime operations](operations/realtime.md).

## Native push devices

- `PUT /devices/{id}` registers or replaces one device using a client-generated UUID and
  `{platform,token,enabled?,previews?}`. Platform is `android` or `ios`; token is 1..4096
  UTF-8 bytes with no whitespace/control characters. iOS tokens must be hexadecimal and are
  stored lowercase. `enabled` defaults to true and `previews` defaults to false. The update
  replaces the prior registration and binds it to the bearer session used for the request.
  An identical PUT from that same session preserves the timestamp and pending delivery.
- The 200 response is `{id,platform,enabled,previews,updated_at}` and never returns the push
  token. A session revocation removes its registrations; an expired session cannot receive
  notifications. A token already registered to an active session on another owner/device
  returns 409; a registration tied to an expired session can be reclaimed. At most 32
  devices per owner; excess registration returns 429 `device_limit`.
- `DELETE /devices/{id}` -> 204 for an owned device; another owner's or missing ID is 404.
- Successful text/image completions enqueue a bounded notification only for enabled,
  session-valid devices with the corresponding provider configured. The default alert is
  `Cindy replied` with `Open your conversation.` for text or `Your image is ready.` for an
  image. Opt-in previews include at most 160 Unicode characters from the assistant reply, or
  the private attachment ID for a generated image; user prompts and titles are never included.
  Data includes a `{app_scheme}://chat/{chat_id}?generation={generation_id}` deep link,
  `OPEN_CHAT` and text-input `REPLY` action descriptors; text replies also offer `COPY_REPLY`.
  The scheme follows the validated `AUTH_APP_SCHEME`, defaulting to `smartcindy` when
  unconfigured. Development uses `smartcindy-dev`. No token or public media URL is
  sent to clients through these payloads. This is best-effort notification delivery, not a
  replacement for authenticated history/status APIs.

## Private images

- `POST /attachments` with raw body and `Content-Type: image/jpeg|image/png|image/webp`
  -> 201 `{id,url,mime,width,height}`. Not multipart or JSON; no arbitrary remote URLs.
- `GET /attachments/{id}` -> original authenticated bytes, no-store, nosniff. Owner only.
- `DELETE /attachments/{id}` -> 204 only if never attached; referenced/retained images 409.

Input and provider output undergo the same actual decode, MIME/magic and dimension checks:
8 MiB/image, 20 megapixels; bounded blocking decode concurrency 2, 160 MiB allocation
limit per decoder. Storage quota is 64 MiB/owner, transactionally reserved against the
user row; generated images fail durably if quota exhausted. Bytes live in PostgreSQL bytea
for this slice and are never included in lists. URLs are private relative API resource
URLs, never provider URLs. No thumbnails implemented; clients should not assume separate
thumbnail bytes. An attachment previously attached is readable only while at least one
of its referencing chats is live. Sharing within an owner preserves access if any live
reference survives; solely deleted-chat images become 404 immediately. Upload-only
orphans remain readable/deletable by their owner. Deleting a chat removes its attachment references and transactionally purges affected
images with no surviving references, reclaiming their storage quota. Shared live images
and upload-only orphans are preserved. Transcript soft deletion is access revocation, not
complete account erasure. Operator-reviewed orphan cleanup and transcript retention
scheduling remain pending.

## Selected provider subset and compatibility gates

DeepSeek uses server-owned `deepseek-flash`, `/chat/completions`, streamed usage,
`thinking.type=disabled`, data-URL photos and assistant text. Prior generated assistant
images are explicitly described as images whose pixels are not supplied; they are not
fabricated into vision context. SSE parser accepts fragmented bytes, CRLF and multiline
data with 64 KiB frame cap; reasoning_content is never returned/persisted. Only stop +
[DONE] is successful. EOF without DONE, length/content_filter/tool_calls finishes and
malformed frames fail honestly, including after partial output. No raw provider errors.

BFL uses `/v1/flux-2-klein-4b`, x-key and 1024x1024 output. Image-output requests require
nonblank current-user text. Up to four current-user attachments are encoded as private
base64 `input_image`, `input_image_2` through `input_image_4`; historical photos are not
implicitly editing references. Generation and editing return the same private image API.
The returned polling URL is used without reconstruction. It must be HTTPS/443 on api.bfl.ai
or api.<one-region-label>.bfl.ai, without credentials/fragment; result downloads allow
only delivery.<one-region-label>.bfl.ai and carry no authentication. Redirects are disabled.
All other origins fail closed. Explicit library-only loopback simulation permits only its
configured origin; CLI never enables it and clients cannot select provider URLs.

Official contracts checked 2026-10-29: [DeepSeek completion/stream/vision](https://api-docs.deepseek.com/api/create-chat-completion/),
[BFL endpoint/schema](https://api.bfl.ai/openapi.json), [BFL delivery and polling](https://docs.bfl.ai/quick_start/generating_images),
[BFL base64 image editing](https://github.com/black-forest-labs/skills/blob/master/skills/bfl-api/references/endpoints.md).
These source checks and local protocol tests do not establish live paid-provider compatibility.
Capped live text, vision, generation and editing checks remain a release gate.

## Future Flutter consumer (not implemented here)

Use Flutter ListView.builder/SliverList lazy widgets, not browser virtual-DOM terminology.
Stable message IDs, older-page prepend/reverse pagination preserving scroll anchor,
incremental SSE deltas (no full-history reread), deduplicated IDs, separate cursor/cache
per selected branch, bounded message window/image cache and secure bearer storage are
native acceptance requirements. Generate consumer types from this real contract after
review. Native memory/project controls, device/emulator journeys, iOS/macOS, live AI
recall/latency, research, email verification/recovery, billing and operator
retention/reconciliation tooling remain separate pending work.

## Machine-readable contract

The reviewed v1 schema is [openapi.json](openapi.json). Native consumer generation must use
that contract, with the behavioral/cursor/stream semantics in this document. Device code
must not assume that arbitrary provider fields are part of the product API.
