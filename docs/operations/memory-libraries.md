# Memory library integration review

Reviewed 2026-10-29 for the memory/context implementation.

## Reviewed upstream identities

- `chrischabot/mem0-rs`, commit `13f67dbe9cf122e04beac384f4467cc16263c1ad`
  ([core manifest](https://github.com/chrischabot/mem0-rs/blob/13f67dbe9cf122e04beac384f4467cc16263c1ad/crates/mem0-core/Cargo.toml),
  [pipeline](https://github.com/chrischabot/mem0-rs/blob/13f67dbe9cf122e04beac384f4467cc16263c1ad/docs/how-it-works.md)). Apache-2.0; `mem0-core` directly depends on bundled rusqlite.
- [`rig-memory` 0.45.0](https://docs.rs/rig-memory/0.45.0/rig_memory/), including
  `Conversation`-based recent-message views, summary splices, and session log storage.
  The public API was reviewed, but this release is not a SmartCindy dependency.

## Integration result

The selected engines remain deferred; neither is added merely to satisfy a package list.
Their current complete pipelines overlap SmartCindy's SQL transcript and derived-state
lifecycle. Mem0 inferred add performs retrieval, extraction, embeddings, persistence,
SQLite history, and optional entity work. Its extraction errors may become empty results.
That does not satisfy our explicit failed-job reporting or a single PostgreSQL transaction
for memory epoch, source ownership, cancellation and publication.

Using ephemeral SQLite would remove a persistent extra store, but would still require
custom provider/store adapters and would not make the built-in pipeline transactional.
Rig session autosave likewise must not become another transcript authority. A stateless
helper can be adopted later only if it removes more code than its adaptation adds.

The implemented boundary therefore sends one bounded structured request through the
existing DeepSeek HTTP client, validates exact user-source excerpts, and publishes only
through fenced SQL transactions. It combines summary and personal fact extraction in
one background call per eight successful text turns. PostgreSQL and the private Bekko
process handle storage and retrieval without hosted memory services.

This choice is recorded in [ADR 0007](../decisions/0007-memory-context.md). It changes the
timing and integration of the selected libraries, not the provider or storage authority.
Reconsider the engines when scoped deletion, source attribution, metering, cancellation,
and no duplicate model submissions can all be preserved with tested adapters.
