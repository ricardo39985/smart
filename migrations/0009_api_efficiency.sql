-- Optional creation request identity preserves legacy clients while making retries durable.
ALTER TABLE chats
    ADD COLUMN create_request_id uuid,
    ADD COLUMN create_fingerprint bytea,
    ADD CONSTRAINT chat_create_fingerprint CHECK (
        (create_request_id IS NULL AND create_fingerprint IS NULL)
        OR (create_request_id IS NOT NULL AND create_fingerprint IS NOT NULL AND octet_length(create_fingerprint) = 32)
    ),
    ADD CONSTRAINT chat_create_request UNIQUE(owner_id, create_request_id);
