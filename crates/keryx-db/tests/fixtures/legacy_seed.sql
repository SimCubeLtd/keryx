-- Rows a v0 database can hold, exercising every v0 column: a live draft with
-- two versions and repository provenance, a soft-deleted draft, a disabled
-- draft, and git_dirty as NULL, 0 and 1.
INSERT INTO drafts (id, title, description, current_version_id, repo_org, repo_name, repo_host, created_at, updated_at, deleted_at, disabled_at, disabled_reason) VALUES
  ('livedraft001', 'Live plan', 'Two versions', 'verLive2aaaaaaaaaaaa', 'SimCubeLtd', 'keryx', 'github.com', '2026-01-01T10:00:00.000Z', '2026-01-02T10:00:00.000Z', NULL, NULL, NULL),
  ('deleted00001', 'Deleted plan', NULL, 'verDeleted1aaaaaaaaa', NULL, NULL, NULL, '2026-01-03T10:00:00.000Z', '2026-01-03T10:00:00.000Z', '2026-01-04T10:00:00.000Z', NULL, NULL),
  ('disabled0001', 'Disabled plan', 'Off for now', 'verDisabled1aaaaaaaa', 'acme', 'widgets', 'gitlab.com', '2026-01-05T10:00:00.000Z', '2026-01-05T10:00:00.000Z', NULL, '2026-01-06T10:00:00.000Z', 'Superseded');
INSERT INTO draft_versions (id, draft_id, version_number, object_key, content_hash, file_size, created_at, source_ip, user_agent, cli_version, git_branch, git_commit_sha, git_commit_subject, git_dirty, original_filename, has_inline_script, external_image_hosts) VALUES
  ('verLive1aaaaaaaaaaaa', 'livedraft001', 1, 'drafts/livedraft001/verLive1aaaaaaaaaaaa.html', 'hash-live-1', 120, '2026-01-01T10:00:00.000Z', '10.0.0.1', 'keryx-cli/0.1.0', '0.1.0', 'main', 'aaa111', 'first', 0, 'plan.html', 0, '[]'),
  ('verLive2aaaaaaaaaaaa', 'livedraft001', 2, 'drafts/livedraft001/verLive2aaaaaaaaaaaa.html', 'hash-live-2', 5000000000, '2026-01-02T10:00:00.000Z', '10.0.0.2', 'keryx-cli/0.2.0', '0.2.0', 'feat/x', 'bbb222', 'second', 1, 'plan.html', 1, '["img.example.com","cdn.example.org"]'),
  ('verDeleted1aaaaaaaaa', 'deleted00001', 1, 'drafts/deleted00001/verDeleted1aaaaaaaaa.html', 'hash-deleted-1', 50, '2026-01-03T10:00:00.000Z', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 0, '[]'),
  ('verDisabled1aaaaaaaa', 'disabled0001', 1, 'drafts/disabled0001/verDisabled1aaaaaaaa.html', 'hash-disabled-1', 75, '2026-01-05T10:00:00.000Z', '10.0.0.3', NULL, '0.3.0', 'main', 'ccc333', 'third', NULL, NULL, 0, '[]');
