# Native push delivery

The Rust backend registers Android/iOS devices and queues completion alerts after
the saved text or image result commits. PostgreSQL owns this outbox. FCM HTTP v1
and APNs HTTP/2 adapters are implemented; both are disabled by default. A configured
capability means configuration passed validation, not that device delivery was
tested or that the native UI is implemented. There is no notification relay vendor.

## Configuration

Operators provide protected files outside the repository:

- `FCM_SERVICE_ACCOUNT_FILE`: Google service-account JSON containing `type`,
  `project_id`, `client_email`, and `private_key`, with FCM permission for the project.
- `APNS_TEAM_ID`, `APNS_KEY_ID`, `APNS_BUNDLE_ID`, `APNS_PRIVATE_KEY_FILE`: all required
  together; the file contains the APNs `.p8` signing key.
- `APNS_ENVIRONMENT`: explicitly `sandbox` or `production` when APNs is configured.

The CLI requires private, bounded credential files. It never accepts a provider
URL override. Library tests can explicitly select a literal loopback HTTP origin;
that simulator is not available through the production environment configuration.
Do not print tokens, signing keys, authorization headers, or provider response bodies.

FCM uses a service-account RS256 assertion and a cached OAuth access token, refreshed
before expiry. APNs uses an ES256 token refreshed after 50 minutes. Delivery uses
fixed provider origins, connection reuse, no redirects, a 10-second timeout per
request, and a 32 KiB response limit. No model calls are used for notifications.

## Registration and preferences

Authenticated `PUT /v1/devices/{client_uuid}` accepts
`{"platform":"android","token":"...","enabled":true,"previews":false}`;
`platform` may also be `ios`. Omitted preferences reset to those defaults. The
response contains only `id`, `platform`, `enabled`, `previews`, and `updated_at`.
An identical PUT from the same session preserves the record and queued delivery.
A changed registration advances its version and cancels old pending deliveries.

Tokens are 1–4096 bytes without whitespace or control characters; APNs tokens
must be hexadecimal and are normalized to lowercase. The JSON body limit is 16 KiB.
An owner may register at most 32 devices. Token hashes are globally unique, so a
token already bound to another device/owner returns generic `409 conflict` rather
than transferring it. A registration bound to an expired session is reclaimed
when that exact token is registered again. Invalid input returns `400`; exceeding the device limit
returns `429 device_limit`. Registration does not prove the token belongs to the
physical client; the platform provider and native registration flow provide it.

`DELETE /v1/devices/{id}` returns `204` for an owned registration and `404` for a
missing/foreign registration. Devices belong to the registering session: logout
removes its registrations and queued deliveries, and expired sessions are excluded
at enqueue, claim, and delivery. The app should unregister before switching accounts
and register token refreshes under its current session. Re-register after login.

`GET /v1/capabilities` includes `push`, `push_android`, and `push_ios` booleans.
Registration remains available when providers are disabled; no delivery is queued
for an unconfigured platform. `enabled:false` suppresses subsequent deliveries.

## Rich payload and privacy

Default alerts contain no conversation text or title. Their title is “Cindy replied”
and body is “Open your conversation.” or “Your image is ready.” With `previews:true`,
text alerts include at most 160 characters of the saved assistant reply. Image
alerts carry only an authenticated attachment ID. No user prompt, public image
URL, session credential, or signed bearer URL is included. Opting in allows the
platform push service and lock screen to process/display the preview.

Both platforms receive string metadata `type=conversation_updated`, `chat_id`,
`generation_id`, and `deep_link={app_scheme}://chat/{chat}?generation={generation}`.
The scheme follows the validated authentication app scheme (`smartcindy-dev` for
development, `smartcindy` for production/default), keeping notifications in the
correct app installation.
`actions` is a JSON-encoded array describing `OPEN_CHAT`, text-input `REPLY`, and
`COPY_REPLY` for text completions. `attachment_id` appears only for opted-in image
previews. These are client action descriptors, not provider-rendered buttons.
Payload serialization is checked against 4 KiB; credentials and device routing
tokens are not content payload fields.

Android receives notification title/body, `conversation_updates` channel,
`OPEN_CHAT` click action, a chat-specific notification tag/collapse key, high
priority, and a one-hour TTL. High priority applies to these visible user-requested
completion alerts, not silent background polling. The client must create the channel and implement
notification rendering, Android RemoteInput, and action handlers. FCM/Android
may collapse, delay, or omit delivery according to platform state and policy.

iOS receives an `aps` alert with title/subtitle/body, default sound,
`CHAT_REPLY`/`CHAT_IMAGE` category, chat thread ID, active interruption level, and
`content-available:1`. Opted-in attachment alerts additionally set
`mutable-content:1`. Requests use alert push type, priority 10, a one-hour expiry,
and a chat-specific collapse ID. There is no fabricated unread badge or privileged
time-sensitive/critical interruption setting.

The sibling Flutter/native app still needs notification permissions, token refresh
registration, foreground suppression, deep-link handling, category/action setup,
and notification service extensions. A service extension must obtain a securely
shared session and fetch private attachments through the authenticated API; missing
authorization should leave a generic alert. Reply actions use native text input,
fetch the completed generation's assistant message, and submit the existing turn
API with a fresh request UUID and that parent. Open the authenticated app when a
secure session is unavailable. Copy actions fetch the reply securely. Never put
typed replies or fetched content in notification analytics. No native action,
foreground behavior, background execution, or image rendering is claimed complete.

## Delivery and bounded work

An outbox row is unique per generation/device. Enqueue runs in the result transaction;
network delivery runs afterward without database locks. Each process dispatches one
push at a time; database leases allow multiple processes without claiming the same
row concurrently. A recipient's queued notifications for the same chat are
coalesced. At most 1,024 queued/sending notifications per owner are admitted.
Enqueue protects selected device rows through insertion, so concurrent unregister
or logout cannot invalidate the outbox foreign key and roll back a saved answer.

Leases last 30 seconds. Retryable errors receive at most three total attempts with
exponential backoff starting at one minute, provider Retry-After delay, and small
per-job jitter. Unknown outcomes may be retried, so delivery is best effort and
duplicates remain possible; collapse identifiers and native generation-ID dedupe
reduce their effect. A successful provider response means provider acceptance,
not delivery/read receipt. Failed notification delivery never retries a paid model
generation or changes the saved answer.

Known invalid/unregistered tokens are disabled. Jobs recheck device version,
session validity, and chat deletion before delivery. Changed preferences and
logout cancel pending work; an already accepted provider request cannot be recalled.
Queued work expires after one hour. Terminal records clear optional preview content
and are deleted in bounded batches after seven days. Maintenance and queue claims
are bounded; native foreground presence is not inferred by the backend.

## Verification and remaining gates

`tests/push_http.rs` exercises real PostgreSQL migrations, public registration and
completion flows, local FCM/OAuth/APNs protocol simulators, owner/session isolation,
device limits, token retirement, and preview privacy. Its normalization/ownership
property uses 8 cases with seed `73591245`. Only deliberately public synthetic
RSA/EC fixtures are used. The simulator does not establish provider compatibility.
The concurrent device-removal regression uses a PostgreSQL row-lock barrier to
coordinate public unregister and completion requests, then checks saved history.
Another recovery scenario locks an expired delivery while other public completions
proceed, then verifies that its fresh replacement lease survives maintenance.

Focused compile checking passed locally. Docker/socket restrictions prevented
the PostgreSQL contract run; identical-PUT and expired-session recovery regressions
were added before their fixes but could not establish red/green runtime results.
The completion/unregister regression was likewise added before its row-lock fix;
its runtime red/green check remains blocked by the same Docker restriction.
The maintenance/lease regression was added before locking maintenance candidates;
its runtime verification has the same restriction. Candidate selection locks only
outbox rows and skips concurrent claims, preventing stale lease/status overwrites.
Focused mutation checking, real provider smoke checks with operator test accounts,
and Android/iOS device acceptance remain gates. iOS requires macOS/Xcode.

Before release, test retries/lease takeover and simultaneous device changes against
real PostgreSQL, then validate token refresh, foreground/background/terminated
delivery, lock-screen privacy, direct reply, secure image loading, logout, and
deep links on both platforms. Do not treat compile or simulator acceptance as a
native delivery pass.

Provider references:

- [FCM HTTP v1 authorization and delivery](https://firebase.google.com/docs/cloud-messaging/send/v1-api)
- [FCM errors and backoff](https://firebase.google.com/docs/cloud-messaging/error-codes)
- [APNs token connections](https://developer.apple.com/documentation/usernotifications/establishing-a-token-based-connection-to-apns)
- [APNs request headers and payloads](https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns)
- [Apple notification payload keys](https://developer.apple.com/library/archive/documentation/NetworkingInternet/Conceptual/RemoteNotificationsPG/PayloadKeyReference.html)
