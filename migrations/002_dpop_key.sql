-- Migration: Add ephemeral DPoP key to oauth_state
-- AT Protocol requires DPoP key != client JWKS key
-- Apply with: psql -d kerai -f migrations/002_dpop_key.sql

ALTER TABLE kerai.oauth_state ADD COLUMN IF NOT EXISTS dpop_key TEXT NOT NULL DEFAULT '';
