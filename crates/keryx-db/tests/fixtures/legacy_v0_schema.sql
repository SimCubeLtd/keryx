-- The schema as the first Keryx release created it (commit a04b077, re-indented):
-- no repository columns on versions, no snoozed_until, no push tables,
-- user_version 0. Do not edit: parity is judged against history.
CREATE TABLE IF NOT EXISTS drafts (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    current_version_id TEXT,
    repo_org TEXT,
    repo_name TEXT,
    repo_host TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    disabled_at TEXT,
    disabled_reason TEXT
);

CREATE TABLE IF NOT EXISTS draft_versions (
    id TEXT PRIMARY KEY,
    draft_id TEXT NOT NULL REFERENCES drafts(id),
    version_number INTEGER NOT NULL,
    object_key TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    source_ip TEXT,
    user_agent TEXT,
    cli_version TEXT,
    git_branch TEXT,
    git_commit_sha TEXT,
    git_commit_subject TEXT,
    git_dirty INTEGER,
    original_filename TEXT,
    has_inline_script INTEGER NOT NULL DEFAULT 0,
    external_image_hosts TEXT NOT NULL DEFAULT '[]',
    UNIQUE (draft_id, version_number)
);

CREATE INDEX IF NOT EXISTS draft_versions_draft_id_idx ON draft_versions(draft_id);
CREATE INDEX IF NOT EXISTS drafts_updated_at_idx ON drafts(updated_at);
