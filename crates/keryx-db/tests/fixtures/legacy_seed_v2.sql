-- Rows only a user_version 2 database can hold: a snoozed draft, and the push
-- tables every release since 0.5.0 creates, with a pending delivery.
INSERT INTO drafts (id, title, description, current_version_id, repo_org, repo_name, repo_host, created_at, updated_at, snoozed_until) VALUES
  ('snoozed00001', 'Snoozed plan', NULL, 'verSnoozed1aaaaaaaaa', NULL, NULL, NULL, '2026-01-07T10:00:00.000Z', '2026-01-07T10:00:00.000Z', '2099-01-01T08:00:00.000Z');
INSERT INTO draft_versions (id, draft_id, version_number, object_key, content_hash, file_size, created_at, repo_org, repo_name, repo_host, has_inline_script, external_image_hosts) VALUES
  ('verSnoozed1aaaaaaaaa', 'snoozed00001', 1, 'drafts/snoozed00001/verSnoozed1aaaaaaaaa.html', 'hash-snoozed-1', 90, '2026-01-07T10:00:00.000Z', 'SimCubeLtd', 'synapse', 'github.com', 0, '[]');
INSERT INTO push_subscriptions (id, endpoint, p256dh, auth, events, created_at, updated_at) VALUES
  ('subAaaaaaaaaaaaaaaaa', 'https://push.example.com/send/abc', 'p256dh-key', 'auth-secret', '["published","revised"]', '2026-01-08T10:00:00.000Z', '2026-01-08T10:00:00.000Z');
INSERT INTO notification_events (key, kind, draft_id, title, body, target, created_at) VALUES
  ('published:livedraft001:verLive1aaaaaaaaaaaa', 'published', 'livedraft001', 'Live plan', 'Published', '/d/livedraft001', '2026-01-08T11:00:00.000Z');
INSERT INTO notification_deliveries (event_key, subscription_id, attempts, next_attempt_at) VALUES
  ('published:livedraft001:verLive1aaaaaaaaaaaa', 'subAaaaaaaaaaaaaaaaa', 2, '2026-01-08T12:00:00.000Z');
