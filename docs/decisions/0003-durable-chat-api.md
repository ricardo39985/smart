# 0003: Durable native bearer chat, fenced jobs and private PostgreSQL images

Date: 2026-10-29

Status: Accepted for this authorized implementation slice; build, behavior, mutation and
live provider compatibility verification remain gates, not implied by this status.

## Context

The first native backend needs authenticated durable chat, explicit branches, resumable
streaming and private photos/generated images. The previous architecture selected durable
sessions and memory libraries but had no implemented protocol. Native clients need an
explicit bearer contract rather than browser cookie-only tower-sessions assumptions.
Authentication, request identity, transaction fences and cancellation require stronger
reasoning/review than mechanical scaffold work. No app or hosted CI is in scope.

## Decision

One Rust library package exposes Config/Application, router, direct migration operation,
operator provisioning and bounded worker lifecycle. CLI is assembly only. Runtime pooling
and direct migration URLs are separate; startup never implicitly migrates through PgBouncer.
SQLx ordered migrations create PostgreSQL 18 state and the vector extension. SQL remains
the sole transcript, session, request and job authority.

Operator-created accounts use Argon2id. Login returns a random 256-bit opaque native bearer;
PostgreSQL stores SHA256 hashes and expiry only. Revocation is checked on each request and
repeatedly on long streams. No trusted user header, public signup, password argv, cookie-only
session dependency, wildcard CORS, or content/credential logging. Login hashing and attempts
are process-bounded. A coordinated edge abuse policy is still a launch gate.

Immutable messages form a parent-linked DAG, with bounded immutable ancestor IDs for cheap
cursor ancestry checks. Explicit assistant parent is mandatory for continuation; edits are
new user siblings, regenerations new assistant siblings. Current head is convenience state,
not permission to rewrite history. Lists use immutable sequences, owner/filter-scoped
versioned opaque cursors, snapshot high watermark and bounded keyset/ancestor reads. Chat
creation and initial watermark acquisition serialize briefly on owner row so uncommitted
lower sequences cannot appear later in an in-flight insertion snapshot.

Admission locks a global advisory key, owner and chat, checks owner-wide UUID/fingerprint
idempotency and queue limits, creates user/job/accepted event atomically. One active job per
chat is additionally enforced by a partial unique index. Provider submission is never retried
automatically. Two workers/process use short SKIP LOCKED claims and fenced UUID leases.
Expired running jobs fail with unknown provider outcome instead of another paid request.
Terminal publication checks live chat and lease while committing message, usage, status and
events together. Cancellation/deletion and completion serialize; terminal states never change.
No database connection/lock spans provider IO, decode, SSE waiting or yielding.

SSE is a replay view of committed events, not the provider socket. Text is coalesced, bounded,
and final successful history is exactly the concatenated durable deltas. Polling rechecks
session/ownership/deletion, uses bounded event pages and releases connections before waiting.
Disconnect never resubmits. Partial failures are visible as failures, not saved fake answers.

Private JPEG/PNG/WebP bytes use PostgreSQL bytea in this slice, with transactionally protected
64 MiB owner quota and validated bounded blocking decode. This intentionally trades database
size/backup/egress costs for atomic metadata/quota/publication and consistent privacy. A live
reference is necessary after first attachment; deleting the sole referencing chat immediately
removes access, while same-owner shared live references retain it. Deleting a chat also removes its references and purges affected images that have no live
references, reclaiming quota atomically. Upload-only orphans and shared live images remain.
Reviewed transcript retention, orphan cleanup and eventual object-storage boundaries are pending. No public/provider image URL is a substitute for owned authenticated bytes.

DeepSeek Flash and BFL FLUX.2 Klein 4B remain selected, server-configured providers. Reqwest
is shared, Rustls, bounded, redirect-free and retry-free for paid submits. DeepSeek supports
selected ancestry text/photos and excludes reasoning output. Image generation uses the current prompt; photo editing passes up to four private current
inputs as base64 after official input_image contract verification.
BFL polling/delivery URLs are narrowly allowlisted, failing closed on unknown hosts. Official contracts were inspected; capped live checks must still confirm
current endpoint/model/vision/editing behavior;
local protocol simulation is not evidence of live compatibility.

Rig, rig-memory and the selected pinned mem0-rs are **deferred**, not discarded/replaced.
Introduce them with the memory/context slice after adapter, auxiliary SQLite history lifecycle,
purge, backup and reconciliation review. They must not become a second transcript authority.
No speculative dependencies are added for memory, billing, signup/email, embeddings or research.

## Consequences

See [API](../api.md) for exact paths, caps, errors and native consumer responsibilities. Multiple
processes can safely claim/fence jobs but worker count and login rate bounds are per-process;
operators must bound replicas. Running paid outcomes require reconciliation after process loss;
no system can truthfully promise cancellation reverses a remote charge. Soft deletion is access
revocation, not completed physical erasure. Limits are deliberately conservative and require
capacity measurement before launch. Device journeys remain in the sibling Flutter repository.

## Verification

Public real-listener HTTP tests use disposable PostgreSQL 18/pgvector pinned to the supplied
Linux amd64 digest and production migrations; only paid HTTP protocols are simulated. Tests
cover authentication/ownership, durable retry/replay/history, insertion snapshots, branch
editing/regeneration, photos/private output, cancellation/deletion and malformed streams.
Failure injection sets an expired running lease through SQL solely to model process death;
assertions use public API and no repeated paid submit. Public cursor/limit properties use 24
fixed-seed cases. Supervisor must resolve lockfile/compile compatibility, run focused suites,
review SQL races/resource caps, mutation-check ownership/order/fences and verify live provider
contracts before claiming acceptance. No checks have been run by the restricted coding agent.
