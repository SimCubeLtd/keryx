-- What the old user_version 2 step did: append snoozed_until, bump the pragma.
ALTER TABLE drafts ADD COLUMN snoozed_until TEXT;
PRAGMA user_version = 2;
