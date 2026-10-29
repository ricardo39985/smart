ALTER TABLE sessions ADD CONSTRAINT sessions_owner_token UNIQUE(user_id,token_hash);
CREATE TABLE devices (
    owner_id uuid NOT NULL REFERENCES users(id),
    id uuid NOT NULL,
    session_hash bytea NOT NULL,
    platform text NOT NULL CHECK(platform IN ('android','ios')),
    token text NOT NULL CHECK(octet_length(token) BETWEEN 1 AND 4096),
    token_hash bytea NOT NULL UNIQUE CHECK(octet_length(token_hash)=32),
    enabled boolean NOT NULL DEFAULT true,
    previews boolean NOT NULL DEFAULT false,
    version bigint NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(owner_id,id),
    FOREIGN KEY(owner_id,session_hash) REFERENCES sessions(user_id,token_hash) ON DELETE CASCADE
);
CREATE TABLE push_outbox (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL,
    device_id uuid NOT NULL,
    device_version bigint NOT NULL,
    chat_id uuid NOT NULL REFERENCES chats(id),
    generation_id uuid NOT NULL REFERENCES generations(id),
    payload jsonb NOT NULL CHECK(octet_length(payload::text)<=4096),
    status text NOT NULL CHECK(status IN ('queued','sending','sent','failed','cancelled')),
    attempts integer NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
    lease uuid,
    lease_until timestamptz,
    next_attempt timestamptz NOT NULL DEFAULT now(),
    error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY(owner_id,device_id) REFERENCES devices(owner_id,id) ON DELETE CASCADE,
    UNIQUE(generation_id,owner_id,device_id)
);
CREATE INDEX push_queue ON push_outbox(next_attempt,created_at) WHERE status='queued';
CREATE INDEX push_leases ON push_outbox(lease_until) WHERE status='sending';
CREATE INDEX push_device ON push_outbox(owner_id,device_id);
