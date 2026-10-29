# Projects implementation plan and contract

User-authorized backend scope: organize conversations into private projects with creation,
listing, rename, move, detach and non-destructive project deletion. Native UI integration,
shared projects, project-specific instructions and automatic cross-chat project context are
outside this contract. A project is an organization boundary, not an implicit memory scope.

## Public behavior

All routes require a valid native session and retain the standard JSON error envelope.

| Route | Input | Result |
| --- | --- | --- |
| `POST /v1/projects` | `{request_id,name}` | 201 new project; 200 exact replay |
| `GET /v1/projects` | `limit` 1–100, default 30; optional cursor | `{items,next_cursor}` |
| `GET /v1/projects/{id}` | Owned live project | Project |
| `PATCH /v1/projects/{id}` | `{version,name}` | Updated Project |
| `DELETE /v1/projects/{id}` | `?version=N` | 204; preserve and detach all chats |
| `POST /v1/chats/{id}/project` | `{version,project_id}`; explicit null detaches | `{id,version,project_id}` |

Project fields: `id`, `name`, `version` (initially 0), `created_at`, `updated_at`. Names
must be nonblank and at most 200 Unicode characters; original whitespace is retained.
Creation IDs are unique per owner and immutable. Exact replay returns the current project;
reusing an ID with a different original name returns 409. Replaying a deleted creation
returns 410 `project_deleted`, never recreates the project. At most 1,000 live projects per
owner are admitted; additional creates return 429 `project_limit`.

Missing/foreign/deleted resources return 404, stale versions 409 and invalid inputs 400.
The move route requires `project_id` even when null; omission is not an accidental detach.
An accepted move increments the chat version, including an explicit move to its current
project. Chat creation optionally accepts `project_id`; chat reads include nullable
`project_id`; existing chat listing accepts an owner-checked project filter. Project filters
must be bound to chat cursors so changing the project during pagination is rejected.

## Storage and concurrency

`0006_projects.sql` adds projects and nullable chat membership. A composite foreign key
enforces equal owners even beneath the HTTP layer. Project creates, moves and deletions
serialize briefly on the owner's row. Project creation and initial listing watermark use
the same lock so uncommitted rows cannot appear in the middle of an existing page.
Pagination uses immutable descending sequences, an upper watermark, and bounded reads.

Deletion is one transaction: detach associated chats, increment their versions, and soft
delete the project. Messages, branches, attachments, generations and saved memory remain.
Chat creation/move validate a live owned project under the same owner lock, preventing a
concurrent deletion from leaving a chat attached to a deleted project. No provider call or
inference occurs in these operations.

## Acceptance

`tests/projects_http.rs` exercises public HTTP against production PostgreSQL/pgvector
Testcontainers migrations: two-owner isolation, duplicate concurrent creation, stale writes,
Unicode name bounds, stable pages, attach/detach, invalid/deleted destinations, and preserved
saved conversation history after project deletion. The text model is a local HTTP protocol
simulator in the preservation journey. Eight generated length cases use seed 73641986.

Required handoff gates: formatting, strict Clippy, compile, focused real-PostgreSQL journeys,
and focused mutation checks of ownership/version/deletion behavior. Runtime tests require
access to Docker; never report a blocked Testcontainers start as a pass. Native acceptance
is separate and remains unimplemented in this backend-only slice.
