CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX generations_user_attempt_lookup
    ON generations(user_message_id, created_at DESC, id DESC);

CREATE INDEX chats_title_trgm
    ON chats USING gin (lower(title) gin_trgm_ops)
    WHERE deleted_at IS NULL;
