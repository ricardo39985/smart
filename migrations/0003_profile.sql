-- Historical names may have been edited locally; never reassign their authority.
ALTER TABLE users ADD COLUMN name_source text NOT NULL DEFAULT 'local'
    CHECK (name_source IN ('local', 'google', 'apple'));
ALTER TABLE account_identities ADD COLUMN verified_email text,
    ADD COLUMN provider_name text,
    ADD COLUMN picture_url text;
ALTER TABLE auth_completions ADD COLUMN provider_name text,
    ADD COLUMN picture_url text;
CREATE INDEX account_identities_verified_email_idx ON account_identities (verified_email)
    WHERE verified_email IS NOT NULL;
CREATE TABLE user_profile_photos (
    user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    bytes bytea NOT NULL CHECK (octet_length(bytes) BETWEEN 1 AND 2097152)
);
