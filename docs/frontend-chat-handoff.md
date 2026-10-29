# Frontend continuation handoff

Current status (2026-10-29): the integration is activated and the development
API/PostgreSQL have moved to Inspiron; Danny remains the build workhorse. See
[local runtime operations](operations/local-development-api.md) and
[move evidence](verification/2026-10-29-local-runtime-move.md). The Flutter client
integration, native push source and subsequent scrolling fixes are committed
through `8d27bea` (APK 0.3.4+7). Its own status and verification records govern
client acceptance. The preparation/patch instructions below are historical;
do not reapply the original patch to the current client checkout.

Rick has authorized completing the frontend integration. Work in
`/home/rick/Documents/Code/smartcindy-app` with write access. Preserve the premium,
casual visual design, responsive images, bounded memory/concurrency and minimum
necessary API calls. Avoid repeated builds, screenshots or infrastructure repair
loops. Read that repository's contributor instructions first.

## Apply the completed work

The implementation is already written; do not recreate it.

- Patch: `/home/rick/Documents/Code/smartcindy/.work/smartcindy-frontend-integration.patch`
- SHA-256: `739962b34fee228baa03a1c91433b26c6234f0872d44b49dbd07c82d76754f7e`
- Original frontend base: `babd6ad`.
- Complete writable source: `/home/rick/Documents/Code/smartcindy/.work/frontend-integration`.
- Backend contract: `/home/rick/Documents/Code/smartcindy/docs/openapi.json`.

Inspect current Git status and preserve any newer frontend edits. The patch passed
`git apply --check` against the unchanged original base. Recheck before applying:

```sh
cd /home/rick/Documents/Code/smartcindy-app
git status --short
git apply --check /home/rick/Documents/Code/smartcindy/.work/smartcindy-frontend-integration.patch
git apply /home/rick/Documents/Code/smartcindy/.work/smartcindy-frontend-integration.patch
```

If the base has changed, reconcile deliberately using the complete writable source;
do not reset user changes or force the patch. Read `docs/live-integration.md` after
applying it. Backend and frontend remain separate repositories.

## Already implemented

Real Chat/Your space/Create entry points, authenticated transport and generated
response validation, streaming/reconnect/cancellation, immutable durable request
recovery, branch/history pagination, responsive private images and bounded caches,
image questions/generation/editing, project CRUD/title search, memory controls and
one coalesced foreground resource-event stream. Session changes dispose private
transports/caches. Models/reasoning remain exclusively backend-controlled.

Do not replace those flows with preview data, full-history requests, background
polling alongside healthy SSE, or automatic retries of unknown paid outcomes.

## Remaining client work

1. Apply/review the patch in the actual app repository.
2. Implement native push permissions, token lifecycle, session-bound registration,
   foreground suppression and authenticated chat deep links. Preserve previews-off
   by default. Complete platform notification actions and secure image handling;
   never put bearer credentials in a URL or notification payload.
3. Use `docs/operations/push.md` and the OpenAPI contract in the backend repository:
   `PUT /v1/devices/{client_uuid}`, `DELETE /v1/devices/{id}`, and platform capability
   booleans. Android uses FCM tokens; iOS backend expects native hexadecimal APNs
   tokens, not an FCM token. Inspect existing native configuration without exposing
   signing/provider secrets. Provider credentials remain operator-managed.
4. Run the retained focused transport/controller regressions on a writable pinned
   SDK, then build Android and run the native core journey against the updated API.
   `integration_test/live_chat_test.dart` requires explicit synthetic account
   configuration; missing configuration intentionally fails. Keep paid-provider
   use within an explicit test budget. iOS acceptance requires macOS/Xcode.
5. Validate send/stop/reopen, branch selection, older history, memory edits,
   projects/search, image-only input and image editing, account switching, and
   foreground/background/terminated push behavior. Record actual failures and fix
   them; don't repeatedly run unrelated suites or capture screenshots by default.

## Backend dependency and truthful evidence

Development API is `https://dev.smartcindy.app`. The last verified deployed build
was the profile slice, not the new integration source. Backend activation and
migrations through 0010 must finish before testing the new routes. See
`docs/operations/native-integration-update.md` in the backend repository. Preserve
the continuously running Cloudflare tunnel.

Full Dart analysis and formatting passed; generated contract consistency passed.
Flutter tests never executed: the launcher could not write the installed SDK cache.
No native/live-provider acceptance or release build is claimed. This session left
the original frontend untouched and did not commit or push either repository.

The existing billing, recovery/erasure, research and reminder product gates are
separate from this integration; do not silently claim those features are complete.
