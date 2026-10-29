CREATE UNIQUE INDEX accounts_email_unique ON users (lower(email));
ALTER TABLE users ALTER COLUMN password_hash DROP NOT NULL;
ALTER TABLE users ADD COLUMN display_name text NOT NULL DEFAULT '';
UPDATE users SET display_name = COALESCE(NULLIF(left(trim(split_part(email, '@', 1)), 80), ''), 'User');
ALTER TABLE users ALTER COLUMN display_name DROP DEFAULT;
ALTER TABLE users ADD CONSTRAINT users_display_name_length CHECK (char_length(display_name) BETWEEN 1 AND 80);
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS created_at timestamptz NOT NULL DEFAULT now();

CREATE TABLE account_identities (
    provider text NOT NULL CHECK (provider IN ('google', 'apple')),
    subject text NOT NULL CHECK (length(subject) BETWEEN 1 AND 255),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (provider, subject),
    UNIQUE (user_id, provider)
);
CREATE TABLE auth_attempts (
    id uuid PRIMARY KEY,
    provider text NOT NULL CHECK (provider IN ('google', 'apple')),
    state_hash bytea NOT NULL UNIQUE,
    nonce text NOT NULL,
    challenge text NOT NULL,
    code_verifier text,
    owner uuid REFERENCES users(id) ON DELETE CASCADE,
    owner_session_hash bytea REFERENCES sessions(token_hash) ON DELETE CASCADE,
    CHECK ((owner IS NULL) = (owner_session_hash IS NULL)),
    expires_at timestamptz NOT NULL,
    claimed boolean NOT NULL DEFAULT false
);
CREATE TABLE auth_completions (
    code_hash bytea PRIMARY KEY,
    attempt_id uuid NOT NULL UNIQUE REFERENCES auth_attempts(id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL,
    provider text NOT NULL,
    subject text,
    email text,
    failure text,
    CHECK ((failure IS NULL AND subject IS NOT NULL) OR (failure IS NOT NULL AND subject IS NULL))
);
CREATE INDEX auth_attempts_expiry ON auth_attempts(expires_at);
CREATE INDEX auth_completions_expiry ON auth_completions(expires_at);
