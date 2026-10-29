CREATE TABLE projects (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 200 AND btrim(name) <> ''),
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    request_id uuid NOT NULL,
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint) = 32),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    UNIQUE(owner_id, id),
    UNIQUE(owner_id, request_id)
);
CREATE INDEX projects_listing ON projects(owner_id, seq DESC) WHERE deleted_at IS NULL;

ALTER TABLE chats ADD COLUMN project_id uuid;
ALTER TABLE chats ADD CONSTRAINT same_owner_project
    FOREIGN KEY(owner_id, project_id) REFERENCES projects(owner_id, id);
CREATE INDEX chats_project_listing ON chats(owner_id, project_id, archived, seq DESC)
    WHERE deleted_at IS NULL;
