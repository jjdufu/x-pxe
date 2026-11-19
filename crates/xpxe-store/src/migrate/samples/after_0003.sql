-- 0003 之后的样例数据。
INSERT INTO users (username, password_hash, role, created_at) VALUES ('sample3', 'x', 'viewer', '2026-01-01T00:00:00Z');
INSERT INTO iso_images (sha256, size, label, volume_id, hybrid, detected_family, detection, uploaded_by, uploaded_at)
  VALUES ('ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12', 1, 's', 'S', 0, 'unknown', '{}', 'admin', '2026-01-01T00:00:00Z');
