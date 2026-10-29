# ADR 0006: Verified profile sources and private custom photos

Status: accepted, implemented and verified for development, 2026-10-29.

## Context

Google's [OpenID Connect documentation](https://developers.google.com/identity/openid-connect/openid-connect) and [claim reference](https://developers.google.com/identity/openid-connect/openid-connect#obtainuserinfo) identify `sub` as the stable identity and describe `name` and `picture` as optional claims. Neither profile fields nor email are authorization to merge accounts. A locally edited name must survive a later provider login; an uploaded photo must remain private.

## Decision

Keep `(provider, subject)` as the only provider identity key. After signed token verification, persist optional, validated Google name and picture and verified email in the proof-bound completion; only a successful exchange updates the identity. Invalid or missing attributes do not erase existing metadata. Existing users migrate to `name_source=local`; newly created provider users use their provider as source and a valid provider name when supplied, otherwise the original email-prefix fallback. Explicit name edits switch source to local. Fresh provider authentication or explicit re-link of the same subject refreshes metadata and the displayed name only when the source matches. The already authenticated, confirmed link journey (including fresh proof) is also how the client may label **Refresh Google profile**; no offline token or webhook is assumed. Missing Google name/picture is normal; clients should use initials when there is no photo.

`users.email` is the permanent, normalized account email and local password login credential. All accounts, including provider-only accounts, display this address. Later provider authentication may refresh verified identity metadata but never changes the account email or its displayed value. Signed-out new accounts must not collide with either reserved user email or an existing verified identity email. Concurrent signup and verified identity-email refresh serialize per normalized address; a refresh colliding with another user's reservation or verified address does not publish that metadata. Provider metadata cannot transfer ownership, select a different user, or change local password login.

The user's 2026-10-29 instruction locks account email and password replacement until verified email delivery exists. Authenticated `POST /v1/account/email-change` and `POST /v1/account/password-change` always return `503 email_delivery_unavailable`, without parsing credentials or mutating account state. Anonymous calls return `401`. The account response advertises both capabilities as `false`; `PATCH /v1/account` still accepts only a display name. This implements an honest disabled boundary, not a mock success or an unfinished password-reset flow. The later email/proof workflow requires its own reviewed contract before enabling these operations.

Custom photos live in a separate user-keyed cascading row, not chat attachments or quota. Authenticated binary uploads decode under shared bounded image capacity, downscale to <=512×512 without enlargement, strip metadata through PNG re-encoding, and atomically replace the row. Reads/deletes only address the authenticated user. The validated provider picture URL is returned as metadata, never fetched by the server; clients choose the private custom image when present.

## Consequences and verification

Provider attributes are best-effort, not guaranteed. Existing locally set names stay local. Uploaded photos are private and never exposed through public provider URLs. Database migrations and real PostgreSQL HTTP regression verification passed; see the profile API verification record. Live provider refresh and iOS acceptance remain separate gates.
