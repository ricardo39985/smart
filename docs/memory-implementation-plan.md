# Memory, personality and efficient context

Scope approved 2026-10-29: implement the backend memory/context section, retaining
PostgreSQL authority, local Bekko/OpenVINO embeddings and hosted generation only.
No deployment or native-client acceptance is implied by backend completion.

## Delivery state

The backend implementation is written for all scopes below. Final source review
and verification are recorded separately from acceptance: formatting, strict lint,
compilation and schema/reference checks can run here; PostgreSQL integration,
mutation checks, real local-model evaluation and live/native push acceptance are
still gated by the documented build/runtime restrictions. The concurrent UI task
owns the coherent profile and disabled email/password-change change set. No mixed
working-tree deployment is performed by this task. See [current status](status.md)
and the [verification record](verification/2026-10-29-memory-backend.md), alongside
the [API audit](verification/2026-10-29-api-audit.md).

## Acceptance

1. Cindy's stable identity and conversational style apply to every text request.
2. Users can list, add, correct, delete and disable their personal memories through
   authenticated, bounded, versioned HTTP APIs. Another user cannot access them.
3. Recent dialogue stays verbatim. Older relevant passages and branch-specific
   summaries supplement it; topic changes and returns do not erase history.
4. Retrieval combines PostgreSQL full-text and local vectors. Embedding failure
   degrades explicitly to keyword retrieval without blocking a paid reply indefinitely.
5. Background extraction/summarization is bounded, metered, source-attributed and
   fenced against corrections, deletion, disablement and concurrent worker takeover.
   No uncertain paid job is resubmitted automatically.
6. Prompt prefixes stay stable where possible. Usage retains cached/uncached token
   counts separately; unknown provider counts remain unknown. No claimed savings
   without live measurements.
7. Public HTTP/PostgreSQL journeys cover ownership, correction, suppression,
   branch selection, topic return, usage and duplicate paid requests. Real local
   embeddings require a separate model-quality and latency check.

## Delivery sequence

- Define migration, lifecycle contract, source/epoch fences and library boundaries.
- Implement private pinned local embedding process and bounded backend client.
- Integrate personality, recent context, hybrid recall and summary selection.
- Add background indexing/extraction, cost accounting and cancellation fences.
- Review integration, run repository checks, focused properties/mutations and
  real-model evaluation where the environment permits. Record remaining gates.

## Additional authorized backend work

- Project creation, listing, rename and deletion, with owned chat assignment and
  removal. Deleting a project detaches its chats rather than deleting their content.
  Existing chat creation, rename and deletion remain supported.
- Editable account name and private profile picture. Third-party sign-in never
  changes the canonical account email or overwrites explicit local profile choices.
- Email/password replacement endpoints explicitly report disabled until verified
  email delivery exists; they must not mutate accounts or pretend to send mail.
- Audit every inbound/outbound route, model payload and native consumer call for
  unnecessary fields, duplicate requests and avoidable payloads. Preserve security
  checks, version fences and context needed for correct answers.
- One-way live invalidations reuse SSE rather than adding a second WebSocket stack;
  clients receive resource IDs/actions and fetch only currently relevant data.
- Android FCM and iOS APNs device registration, durable bounded delivery and rich
  platform payloads. Credentials default absent; private previews are opt-in.
  Native notification rendering/actions/extensions need separate client acceptance.

## Boundaries

Personality is application instruction; memories and recalled passages are untrusted
data. Explicit current user statements override old context. Personal memories
belong to a user; summaries and historical passages belong to a selected branch.
Memory deletion removes saved recall, not the user's original transcript; deleting
the source chat removes its derived data. Disabling memory prevents personal recall
and extraction. User-visible controls are backend contracts; Flutter wiring belongs
to the separate client repository.

The small embedding model is selected, not assumed accurate: evaluate paraphrases,
exact names, ambiguous follow-ups and returning topics before a production claim.
