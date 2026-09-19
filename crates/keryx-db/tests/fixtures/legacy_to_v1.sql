-- What the old user_version 1 step did to a v0 database: append the
-- repository columns, backfill them onto current versions, bump the pragma.
ALTER TABLE draft_versions ADD COLUMN repo_org TEXT;
ALTER TABLE draft_versions ADD COLUMN repo_name TEXT;
ALTER TABLE draft_versions ADD COLUMN repo_host TEXT;
UPDATE draft_versions
SET repo_org = (SELECT d.repo_org FROM drafts d WHERE d.current_version_id = draft_versions.id),
    repo_name = (SELECT d.repo_name FROM drafts d WHERE d.current_version_id = draft_versions.id),
    repo_host = (SELECT d.repo_host FROM drafts d WHERE d.current_version_id = draft_versions.id)
WHERE id IN (SELECT current_version_id FROM drafts WHERE current_version_id IS NOT NULL);
PRAGMA user_version = 1;
