-- Migration: Add access control columns to users table
-- First authenticated user becomes admin; subsequent logins require allowlisting.
-- Apply with: psql -d kerai -f migrations/003_access_control.sql

ALTER TABLE kerai.users ADD COLUMN IF NOT EXISTS is_admin BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE kerai.users ADD COLUMN IF NOT EXISTS is_allowed BOOLEAN NOT NULL DEFAULT false;

-- Bootstrap: mark existing bsky users as admin+allowed
UPDATE kerai.users SET is_admin = true, is_allowed = true WHERE auth_provider = 'bsky';
