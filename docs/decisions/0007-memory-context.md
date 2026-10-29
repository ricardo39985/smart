# 0007: Owner-controlled memory, bounded context, and cache accounting

Date: 2026-10-29

Status: Accepted for the user-authorized memory/context implementation. Verification and
live-provider acceptance are reported separately in `docs/status.md`.

## Context

The first chat slice replays a bounded selected branch and has no durable personality,
personal memory or compaction. The user requested the complete memory/context section,
including local retrieval, topic changes, fast replies, user corrections/deletions and
provider-cache savings. The existing database, provider and embedding choices remain.

## Decision

PostgreSQL remains the single authority. Distinguish stable server-owned Cindy identity,
owner-level editable facts, branch-scoped history, and derived summaries. Retrieved text
is untrusted data, not authority to alter the assistant's instructions. Use recent history
plus bounded hybrid keyword/vector recall and branch-anchored summaries; never inject a
sibling branch's message as if it were part of the selected conversation.

Memory writes require authentication, ownership and optimistic versions. Creation has a
durable owner/request ledger. User controls increment an owner epoch and advance an
extraction watermark. Corrections and deletion invalidate derived state; deletion clears
memory content and embeddings while retaining only the identifiers necessary for durable
idempotency. Disabling memory suppresses cross-chat recall and extraction. It does not
erase the ordinary transcript of the conversation the user is actively viewing.

Completed text replies atomically index their user and assistant text with the answer
transaction, so keyword recall never waits behind live chat work. UTF-8-safe chunks
cover the full text at at most 4000 bytes each. One bounded background worker indexes
four legacy messages per batch, committing every chunk of one message atomically. Missing
chunk and manual-memory embeddings are backfilled in batches of at most sixteen after
local inference recovers.
It checks for active chat work before starting a batch. Successful text replies enqueue a
summary/extraction job at every eighth assistant reply; queues are capped globally and per
owner. Across processes only one background paid job may run. A running job has a fenced
120-second lease and its HTTP request has a 90-second timeout. It sends at most 48 KiB of
source JSON and requests at most 2500 output tokens. This is a bound on model work, not a
subscription entitlement or dollar reservation system.

A combined JSON result contains a summary of supplied source and at most eight durable
facts. Each fact must be an exact user-message excerpt, with a valid source message ID
from the selected ancestry after the control watermark. Assistant suggestions, arbitrary
model-generated paraphrases and invented source IDs cannot become personal facts.
Existing active facts are deduplicated case-insensitively. The same request may propose
superseding an earlier derived fact when the new exact user quote is an explicit correction
of the same attribute. It receives at most 32 untouched derived candidates, bounded to
16 KiB within the existing total request budget. Only provided owner-scoped IDs may be
superseded, and version-zero source-derived status is checked again at publication.
Manual memories and user-edited facts cannot be replaced automatically. A supersession
bumps the owner epoch, advances the extraction watermark, and cancels running text
generations prepared with old memories. The current summary receives the new epoch.
Deleted source IDs prevent later extraction from
restoring old statements. This conservatively suppresses re-extraction of other facts
from the same source message; already saved distinct facts are preserved. Summaries
preserve provenance through an immutable branch
anchor, owner epoch and latest applicable prior summary. The personal-fact watermark does
not erase ordinary chat history from summarization. Oversized unsummarized input
fails explicitly instead of masquerading as a complete summary.

All inference and provider work happens after releasing database connections. Publication
locks owner, chat and job, then checks live chat, enabled memory, matching epoch and live
lease. A stale result cannot restore deleted state. Unknown paid outcomes become terminal
failures rather than automatic resubmissions. Usage, including cached and uncached input,
is stored separately for background work; completed usage can be recorded after cancellation
without allowing memory publication. Streamed chat usage records the same cache fields.

The private Bekko a8m/OpenVINO process supplies normalized 384-dimensional embeddings with
an immutable model revision. Stored embeddings record that revision. Missing or unavailable
inference degrades to keyword retrieval rather than a hosted embedding API. Real inference
compatibility and measured latency must be reported separately from keyword-only tests.

Stable personality instructions begin the prompt; dynamic recall belongs after stable
content. Summaries are refreshed in batches. This gives the provider unchanged prefixes
where practical; no cache-hit guarantee or unmeasured cost saving is claimed.

## Library boundary

Rig, rig-memory and mem0-rs remain selected candidates, but their complete orchestration
engines are **not integrated in this slice**. The pinned-source review found overlapping
transcript/session lifecycles and mem0 SQLite history writes outside our SQL publication
transaction. The direct bounded extraction adapter is the minimal safe implementation
under the requested avoidance of overlapping library responsibilities. No unused library
dependencies or alternative transcript stores are introduced. See the
[review and future adapter gates](../operations/memory-libraries.md).

## Consequences and verification

Ordinary replies do not wait for memory extraction. Keyword indexing commits with the
answer; fresh recent messages also cover the period before extraction or embeddings
complete. Background jobs may lag under sustained chat load.
An already-submitted provider request cannot be recalled or its charge undone by a later
deletion; that deletion prevents stale database publication and use in subsequent requests.

Real PostgreSQL public API journeys must cover ownership, optimistic edits, deletion and
disablement during extraction, source attribution, topic return, duplicate requests,
branch isolation, cache usage and failed providers. Seeded properties and focused mutations
cover substantive guards. Native client memory controls and live model recall/latency are
separate acceptance work; a simulated-provider HTTP pass is not native or live AI acceptance.
