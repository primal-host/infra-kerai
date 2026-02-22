-- Migration: Add OAuth state and config tables
-- Apply with: psql -d kerai -f migrations/001_oauth_tables.sql

CREATE TABLE IF NOT EXISTS kerai.oauth_state (
    state          TEXT PRIMARY KEY,
    code_verifier  TEXT NOT NULL,
    session_token  TEXT NOT NULL,
    handle         TEXT,
    did            TEXT,
    token_endpoint TEXT NOT NULL,
    issuer         TEXT NOT NULL DEFAULT '',
    dpop_nonce     TEXT,
    dpop_key       TEXT NOT NULL DEFAULT '',
    created_at     TIMESTAMPTZ DEFAULT now(),
    expires_at     TIMESTAMPTZ DEFAULT now() + interval '10 minutes'
);

CREATE TABLE IF NOT EXISTS kerai.config (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT now()
);
