# ADR 0010: Chat-head generation lookup and title search

- Date: 2026-10-29
- Status: accepted

## Context

Native clients need to reopen a selected chat branch and recover the generation tied
to its head. Looking up only the chat's newest generation can switch an older branch
to unrelated work. Chat lists also need a bounded, server-side title substring filter
while preserving the existing creation-order cursor model.

## Decision

Add authenticated `GET /v1/chats/{id}/generation`, selecting the current head by
default or validating an explicitly supplied message head. Assistant heads resolve
through their `generation_id`; user heads resolve to the latest attempt ordered by
creation time and ID. Return terminal attempts as stored and never resubmit provider
work. Add an index on `(user_message_id, created_at DESC, id DESC)` for the bounded
latest-attempt lookup.

Add optional `q` to chat listing. Trim and lowercase it, treat blank input as absent,
and match it as a literal substring using escaped `LIKE`. Bind the normalized filter
into chat cursors; continue ordering and snapshot traversal by immutable creation sequence.
Add `pg_trgm` and a partial GIN index on `lower(title)` for substring searches. Keep
one- and two-character searches valid; they may scan candidate chat rows because a
trigram index has no useful token for those short strings.

## Consequences

The generation endpoint returns `{generation: Generation|null}` and does not lock or
hold a database connection across provider work. A missing, foreign, or deleted chat
and a head outside the chat use the existing 404 behavior. Chat cursor payloads gain
an optional search field; existing cursors without it remain valid when no search is
requested. Substring title search uses a literal-escaped `LIKE` pattern, so `%`, `_`
and `!` have no wildcard meaning. Applying the migration requires `pg_trgm` to be
available and the migration role to have extension-install permission on the target
PostgreSQL installation; confirm this on the native Debian release before deployment.

## Verification

The public HTTP contract is documented in `docs/api.md` and `docs/openapi.json`.
Focused HTTP journeys should verify current and explicit heads, terminal attempts,
ownership, literal substring semantics, cursor filter binding, and the trigram index
plan for a representative query against PostgreSQL.
