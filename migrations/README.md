# Database migrations

`0001_durable_chat.sql` creates the initial account/session/chat/message/generation/event/private-image schema and pgvector extension. It has only been applied to disposable test databases; this session has not migrated a persistent development or production database.

Use `smartcindy migrate` with a direct `MIGRATION_DATABASE_URL`, separately from the PgBouncer runtime connection. The migration role needs permission to install vector and create the schema; restrict runtime privileges separately. Ordered SQLx migrations are checked against real PostgreSQL 18/pgvector by Testcontainers. Never edit a migration after persistent deployment; add compatible forward migrations and verify upgrades. Keep locks short and preserve ownership/fencing constraints. See the [testing policy](../docs/testing.md).
