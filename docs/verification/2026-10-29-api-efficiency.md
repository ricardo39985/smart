# Chat API efficiency checks

Scope: optional durable chat-creation request IDs and byte-bounded whole-message pages.

`POST /v1/chats` accepts an optional UUID `request_id`. The owner lock serializes lookup
and insertion. Exact retries return the existing live chat with 200 and its current state;
new chats return 201. A changed creation payload conflicts; a matching deleted chat returns
410. Original title/project input remains fingerprinted even after rename/move or project
deletion. Clients without a request ID retain the original create-every-time behavior.

Messages and branch pages respect both their requested record limit and a 512 KiB target,
reserving 2 KiB for envelope/cursor metadata. Records are never split. A single record is
always returned to ensure progress, even if a future record format exceeds the target.
The first omitted ancestor becomes the message cursor; branch pages retain their original
snapshot watermark and continue below the last included sequence. Page projections omit
ancestor arrays and consume only the needed result prefix.

`tests/api_efficiency_http.rs` adds public PostgreSQL/pgvector scenarios for concurrent
creation retry, ownership, changed payload, post-project-deletion retry, tombstone replay,
large history traversal, byte-limited branches and continuation during a branch snapshot.
Four seeded pagination property cases use seed `73641989`.

Verification at authoring:

- Passed: rustfmt of the changed Rust files and `cargo test --offline --locked --test
  api_efficiency_http --no-run` after the concurrent push adapter compiler error was fixed.
- Passed: `cargo clippy --offline --locked --test api_efficiency_http -- -D warnings`.
- Failed then resolved: the first compile encountered the unrelated push adapter's
  `Vec<Value>::to_string()` error; no changes to the pagination assertions were needed.
- Blocked: public HTTP scenarios and focused mutations require the Docker-enabled
  Testcontainers environment. No runtime behavior or mutation pass is claimed here.
- Not applicable: live paid-provider or native acceptance; the added API tests use a local
  provider protocol simulator and do not prove those surfaces.
