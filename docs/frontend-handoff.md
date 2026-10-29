# Backend handoff for native integration

The memory/context, projects, API-efficiency, realtime, push and DeepSeek-policy
source is ready for the next integration phase. [API guide](api.md) and
[OpenAPI](openapi.json) are the authoritative client contracts. The running Danny
service is still the separately verified profile-only build; these additional
routes require a reviewed migration/build deployment before live client acceptance.

## Wire the native flows

- Chat/project CRUD and assignment: retain IDs, version fields and cursor state;
  supply request UUIDs on creation and every turn. Deleting a project preserves
  its chats. Use whole-record pagination, including byte-limited continuation.
- Chat generation: send only new parts and attachment IDs; consume generation SSE,
  resume from the last processed event and reuse request IDs on uncertain retries.
  Avoid simultaneous status polling while a healthy stream supplies updates.
- Personal memory: implement saved-memory/settings controls, version conflicts,
  correction/deletion and disablement. Context selection, retrieval, personality
  and summaries are server responsibilities. Disabling memory preserves saved
  entries while suppressing recall/extraction.
- Resource changes: consume `/v1/events`, coalesce resource IDs/actions and fetch
  only affected visible data. Honor resync/replay and session revocation. This is
  one-way SSE; no WebSocket client is needed for this contract.
- Push: register session-bound devices and token/preference changes; default
  previews off. Implement permissions, Android channel/actions/RemoteInput, iOS
  categories/service extension, secure attachment loading and deep links. Device
  registration and payload descriptors alone do not render rich notifications.
- Providers: keep model selection and reasoning entirely in the backend. Use
  capability booleans; send no model IDs, reasoning settings or API keys. See
  [DeepSeek configuration](operations/deepseek.md).

Profile routes were implemented and deployed in `ad72b18`; retain that verified
contract. Name/photo updates remain available and email/password replacement
capabilities remain disabled. The [API audit](verification/2026-10-29-api-audit.md)
also records existing redundant account fetches for coordinated client cleanup.

## Acceptance and operations

Formatting, strict all-target Clippy, compiled test targets and documentation checks
passed for the recorded source slices. New database/concurrency/property/mutation
journeys remain blocked in this session by Docker access; real embeddings, live
provider calls and native notification acceptance remain gates. Tests are not
skipped or counted as passed. See [backend verification](verification/2026-10-29-memory-backend.md)
and [DeepSeek verification](operations/deepseek.md#verification).

Before accepting the native integration, execute those affected public contracts
against real PostgreSQL/pgvector on a suitable approved host, resolve failures,
then coordinate the reviewed development deployment and native journeys. Preserve
the continuously running Cloudflare connector; diagnose the origin relay separately
when Danny's API is healthy and public requests return 502.
