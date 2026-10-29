# Native chat API, v1

Implementation contract for this backend slice; verification gates are in [status](status.md).
Product routes start `/v1`; the operational readiness route is `/healthz`. JSON uses UTF-8, UUID strings and UTC RFC3339 timestamps. Optional
response fields are explicit `null`. Unknown request fields, malformed UUIDs/cursors and
out-of-range limits are rejected. Errors: `{"error":{"code":"invalid_request","message":"invalid_request"}}`.
Codes/messages are deliberately generic, never raw SQL/provider responses.

Status meanings: 400 invalid input/unsupported capability/context overflow; 401 absent,
expired or revoked bearer; 404 absent or not owned (indistinguishable); 409 version,
idempotency, reference or active-generation conflict; 413 bytes/quota exceeded; 429
login/admission/decode capacity; 503 provider not configured; 500 storage/internal fault.
Every response is `Cache-Control: no-store`. No wildcard CORS or trusted user-ID header.
Chat, attachments, account profile, logout and capabilities require `Authorization: Bearer TOKEN`.
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
- `GET /account` -> `{id,email,display_name,methods}`. Methods list contains only actual identities (password first, Google then Apple). `PATCH /account {display_name}` updates the name and returns the same account shape. A provider-only account has no usable password.
- `GET /auth/methods` (public) -> `{password:true,google:boolean,apple:boolean}` based on configured credentials; these are not provider-health indicators.
Server configuration: `AUTH_ORIGIN` is exactly `https://dev.smartcindy.app` or `https://smartcindy.app`, paired with `AUTH_APP_SCHEME=smartcindy-dev` or `smartcindy` respectively. Google requires both `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET`. Apple requires `APPLE_CLIENT_ID`, `APPLE_TEAM_ID`, `APPLE_KEY_ID` and an owner-protected `APPLE_PRIVATE_KEY_FILE` read only at startup. Partial configuration fails startup; neither the CLI nor the HTTP client can select provider endpoints. Configure different client IDs, callback registrations and signing material for development and production. Actual provider compatibility and native client acceptance are separate release gates.
- `POST /auth/{google|apple}/start {challenge,link?,confirm?,password?}` -> `{authorization_url,attempt_id}`. Challenge is the base64url/no-padding SHA-256 native PKCE challenge. A five-minute attempt holds a random hashed state and nonce; Google also uses a server-side PKCE verifier. Disabled provider -> 503 `provider_unavailable`. Link mode requires a valid bearer, `confirm:true`, and the existing account's password if it has one; provider-only linking requires a session created in the last five minutes. Missing confirmation -> 400; invalid credentials -> 401. The native client must separately explain and confirm that linking will add a sign-in method to the current account; matching email never authorizes linking.
- Google redirects to `GET /auth/google/callback?state=...&code=...` (or `error`); Apple submits `POST /auth/apple/callback` as `application/x-www-form-urlencoded`. Only an unclaimed matching five-minute state is accepted. After token redemption and ID-token signature/issuer/audience/expiry/nonce verification, the server responds 303 to the **fixed** dev `smartcindy-dev://auth/callback?code=...` or production `smartcindy://auth/callback?code=...` scheme. This code is a two-minute single-use completion proof, **not** a bearer session; no tokens appear in redirect URLs.
- `POST /auth/exchange {code,verifier}` verifies the original S256 challenge under a transaction lock and consumes the completion once. Wrong verifier does not consume it. Successful sign-in -> 200 session shape; successful linking -> 200 account shape plus `linked:true`, without changing the original session. Same-email signed-out collision -> 409 `account_link_required`; subject already linked elsewhere -> 409 `identity_already_linked`; cancelled provider -> 400 `provider_cancelled`. A provider-only account can reauthenticate by its provider subject even if Apple omits email on later grants. No identity is created before successful native proof exchange.
- `GET /capabilities` -> `{text:boolean,image_input:boolean,image_output:boolean,image_editing:boolean}` from
  configured adapter keys, not a live provider health guarantee. Image input is available
  with either vision or editing; image editing follows image-output configuration. No model names/secrets.

Operator provisioning: `smartcindy provision-user EMAIL`, password from bounded stdin
or protected `SMARTCINDY_PROVISION_PASSWORD` environment, never an argument. Passwords
must be 12..1024 bytes. No recovery, email ownership verification, account deletion or billing is implemented.

## Chats and pagination

- `POST /chats {title}` -> 201 chat `{id,title,archived,version,head_id,created_at,updated_at}`;
  title at most 200 Unicode scalar values (empty permitted).
- `GET /chats?limit=30&cursor=...&archived=false` -> `{items,next_cursor}`. Limit 1..100.
  Sort is immutable creation sequence descending, **not updated_at**. A first-page
  owner-scoped high watermark excludes later inserts. Cursors are bounded (1024 chars),
  opaque base64url, typed/versioned and owner/filter scoped; wrong scope is 400.
  No total count/OFFSET. Renaming does not move rows. Archival/deletion changes membership;
  static traversal with concurrent inserts neither repeats nor skips preexisting items.
- `GET /chats/{id}` -> owned chat metadata, including current version and head, without scanning chat pages.
- `PATCH /chats/{id} {version,title?,archived?}` -> updated chat; at least one change required.
- `DELETE /chats/{id}?version=N` -> 204, soft delete and atomic cancellation of active work.
  Deleted resources return 404; stale workers cannot publish. Repeated delete is 404.

Every accepted turn/regeneration and successful publication increments chat version;
PATCH and DELETE compare the version currently returned by chat metadata/listing (stale -> 409).
Chat head changes only through explicit turn/regeneration admission and its fenced result.

## Branches, edits and immutable messages

- `GET /chats/{id}/messages?head_id=UUID&limit=50&cursor=...` ->
  `{items,head_id,next_cursor}`; limit 1..100, newest first. Omitted head selects the current
  head once. Empty chat returns null head, empty items. Cursor pins that selected head,
  even if new turns arrive. A supplied head must agree with cursor head. Wrong owner,
  chat, type or ancestry is rejected. Each page walks at most limit+1 ancestors via primary
  keys; cursor ancestry validation checks the bounded immutable ancestor-ID array, not
  a history scan. The backend does not fetch all history to serve a page.
- Items: `{id,parent_id,role,parts,depth,order,created_at,generation_id,status}`. Depth starts
  at zero, order is immutable creation sequence. Assistant messages identify their
  generation/status; user messages have null generation/status since regenerations can
  produce multiple jobs for one user node. Use acceptance/status APIs to track those jobs.
- `GET /chats/{id}/branches?limit=50&cursor=...` -> `{items,next_cursor}`; leaf heads
  (including unanswered user nodes), indexed creation keyset, snapshot high watermark.
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

Text context is only the selected ancestry, at most 128 messages, conservatively 64 KiB stored
parts JSON and 32 MiB image bytes (repeated references count). Image-output requests use
the current user prompt and its images, with the same message/text budget and at most
32 MiB current input images; historical photos are not automatically image-editing references. Overflow is rejected before
acceptance; no silent memory truncation. No personal memory/research/tool invocation yet.

## Durable generations and SSE

- `GET /generations/{id}` -> `{generation_id,user_message_id,status,result_message_id,error,usage,created_at,updated_at}`.
  States: queued, running, completed, failed, cancelled. Errors are sanitized codes.
  Usage is null when unknown, otherwise {input_tokens,output_tokens,total_tokens}; each
  count is nonnegative or null when not supplied. Raw provider-specific usage fields are not exposed.
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
review. Native devices/emulators, iOS/macOS, memory/research, email verification/recovery, billing and
operator retention/reconciliation tooling are separate pending slices.

## Machine-readable contract

The reviewed v1 schema is [openapi.json](openapi.json). Native consumer generation must use
that contract, with the behavioral/cursor/stream semantics in this document. Device code
must not assume that arbitrary provider fields are part of the product API.
