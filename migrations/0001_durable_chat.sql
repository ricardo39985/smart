CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE users (
    id uuid PRIMARY KEY,
    email text NOT NULL UNIQUE,
    password_hash text NOT NULL,
    image_bytes bigint NOT NULL DEFAULT 0 CHECK (image_bytes BETWEEN 0 AND 67108864),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE sessions (
    token_hash bytea PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id),
    expires_at timestamptz NOT NULL
);
CREATE INDEX sessions_expiry ON sessions(expires_at);
CREATE TABLE chats (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    title text NOT NULL CHECK (char_length(title) <= 200),
    archived boolean NOT NULL DEFAULT false,
    deleted_at timestamptz,
    version bigint NOT NULL DEFAULT 1,
    head_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX chats_listing ON chats(owner_id, archived, seq DESC) WHERE deleted_at IS NULL;
CREATE TABLE messages (
    id uuid PRIMARY KEY,
    chat_id uuid NOT NULL REFERENCES chats(id),
    parent_id uuid REFERENCES messages(id),
    role text NOT NULL CHECK (role IN ('user','assistant')),
    parts jsonb NOT NULL,
    depth integer NOT NULL CHECK (depth BETWEEN 0 AND 128),
    ancestors uuid[] NOT NULL DEFAULT '{}' CHECK (cardinality(ancestors) <= 128),
    seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    replaces_message_id uuid REFERENCES messages(id),
    generation_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(chat_id,id)
);
ALTER TABLE chats ADD CONSTRAINT chat_head FOREIGN KEY (id,head_id) REFERENCES messages(chat_id,id);
ALTER TABLE messages ADD CONSTRAINT same_chat_parent FOREIGN KEY (chat_id,parent_id) REFERENCES messages(chat_id,id);
CREATE INDEX messages_parent_seq ON messages(chat_id,parent_id,seq);
CREATE INDEX messages_chat_seq_desc ON messages(chat_id,seq DESC);
CREATE TABLE generations (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    chat_id uuid NOT NULL REFERENCES chats(id),
    user_message_id uuid NOT NULL REFERENCES messages(id),
    request_id uuid NOT NULL,
    fingerprint bytea NOT NULL,
    output text NOT NULL CHECK (output IN ('text','image')),
    status text NOT NULL CHECK (status IN ('queued','running','completed','failed','cancelled')),
    lease uuid,
    lease_until timestamptz,
    next_seq bigint NOT NULL DEFAULT 1,
    result_message_id uuid REFERENCES messages(id),
    error text,
    usage jsonb,
    provider_receipt text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(owner_id,request_id)
);
ALTER TABLE messages ADD CONSTRAINT message_generation FOREIGN KEY (generation_id) REFERENCES generations(id);
CREATE UNIQUE INDEX one_active_generation ON generations(chat_id) WHERE status IN ('queued','running');
CREATE INDEX generation_queue ON generations(created_at,id) WHERE status = 'queued';
CREATE INDEX generation_leases ON generations(lease_until) WHERE status = 'running';
CREATE INDEX generation_owner_active ON generations(owner_id) WHERE status IN ('queued','running');
CREATE TABLE generation_events (
    generation_id uuid NOT NULL REFERENCES generations(id),
    seq bigint NOT NULL,
    kind text NOT NULL,
    data jsonb NOT NULL,
    PRIMARY KEY(generation_id,seq)
);
CREATE TABLE attachments (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    bytes bytea NOT NULL CHECK (octet_length(bytes) <= 8388608),
    mime text NOT NULL,
    width integer NOT NULL,
    height integer NOT NULL,
    ever_attached boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE message_attachments (
    message_id uuid NOT NULL REFERENCES messages(id),
    attachment_id uuid NOT NULL REFERENCES attachments(id),
    PRIMARY KEY(message_id,attachment_id)
);
CREATE INDEX attachment_references ON message_attachments(attachment_id,message_id);
