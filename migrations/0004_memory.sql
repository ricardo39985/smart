ALTER TABLE users
    ADD COLUMN memory_enabled boolean NOT NULL DEFAULT true,
    ADD COLUMN memory_version bigint NOT NULL DEFAULT 0 CHECK (memory_version >= 0),
    ADD COLUMN memory_after_seq bigint NOT NULL DEFAULT 0 CHECK (memory_after_seq >= 0);

CREATE TABLE memories (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    content text NOT NULL DEFAULT '',
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    source_chat_id uuid REFERENCES chats(id) ON DELETE SET NULL,
    source_message_id uuid REFERENCES messages(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    embedding vector(384),
    embedding_revision text,
    search tsvector GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED,
    seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    CHECK (octet_length(content) <= 2000),
    CHECK ((deleted_at IS NULL AND length(content) > 0) OR (deleted_at IS NOT NULL AND content = '' AND embedding IS NULL AND embedding_revision IS NULL))
);
CREATE INDEX memories_listing ON memories(owner_id, seq DESC) WHERE deleted_at IS NULL;
CREATE INDEX memories_search ON memories USING gin(search) WHERE deleted_at IS NULL;

CREATE TABLE memory_requests (
    owner_id uuid NOT NULL REFERENCES users(id),
    request_id uuid NOT NULL,
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint) = 32),
    memory_id uuid NOT NULL REFERENCES memories(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(owner_id, request_id),
    UNIQUE(memory_id)
);

CREATE TABLE memory_jobs (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    chat_id uuid NOT NULL REFERENCES chats(id),
    head_id uuid NOT NULL REFERENCES messages(id),
    memory_version bigint NOT NULL CHECK (memory_version >= 0),
    status text NOT NULL CHECK (status IN ('queued','running','completed','failed','cancelled')),
    lease uuid,
    lease_until timestamptz,
    usage jsonb,
    error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    UNIQUE(head_id),
    CHECK (
        (status = 'running' AND lease IS NOT NULL AND lease_until IS NOT NULL)
        OR (status <> 'running' AND lease IS NULL AND lease_until IS NULL)
    )
);
CREATE INDEX memory_jobs_queue ON memory_jobs(created_at, id) WHERE status = 'queued';
CREATE INDEX memory_jobs_leases ON memory_jobs(lease_until) WHERE status = 'running';

CREATE TABLE summaries (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    chat_id uuid NOT NULL REFERENCES chats(id),
    through_message_id uuid NOT NULL REFERENCES messages(id),
    memory_version bigint NOT NULL CHECK (memory_version >= 0),
    content text NOT NULL CHECK (octet_length(content) <= 8000),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(through_message_id)
);
CREATE INDEX summaries_chat ON summaries(owner_id, chat_id, created_at DESC);

CREATE TABLE context_chunks (
    message_id uuid NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    chunk_index integer NOT NULL DEFAULT 0 CHECK (chunk_index BETWEEN 0 AND 32),
    owner_id uuid NOT NULL REFERENCES users(id),
    chat_id uuid NOT NULL REFERENCES chats(id),
    text text NOT NULL CHECK (octet_length(text) <= 4000),
    embedding vector(384),
    embedding_revision text,
    search tsvector GENERATED ALWAYS AS (to_tsvector('simple', text)) STORED,
    PRIMARY KEY(message_id, chunk_index)
);
CREATE INDEX context_chunks_owner_chat ON context_chunks(owner_id, chat_id);
CREATE INDEX context_chunks_search ON context_chunks USING gin(search);
