-- 0001 之后的样例数据，用来验证后续迁移不破坏已有数据。
INSERT INTO meta (key, value) VALUES ('master_key_id', '0011223344556677');
INSERT INTO users (id, username, password_hash, role, must_change_password, disabled, created_at)
VALUES (1, 'admin', '$argon2id$v=19$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0$aGFzaA', 'admin', 1, 0, '2026-11-18T00:00:00Z');
INSERT INTO config_objects (id, kind, name, head_version) VALUES (1, 'timeouts', 'default', 1);
INSERT INTO config_versions (object_id, version, parent_version, author_id, created_at, comment, schema, content, content_sha256)
VALUES (1, 1, NULL, 1, '2026-11-18T00:00:00Z', 'init', 1, '{}', '44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a');
INSERT INTO nodes (id, spec_object_id, mac, hostname, state, state_since)
VALUES (1, 1, '52:54:00:00:00:01', 'node01', 'idle', '2026-11-18T00:00:00Z');
INSERT INTO audit_log (id, at, actor, ip, action, target, outcome, detail, prev_hash, hash)
VALUES (1, '2026-11-18T00:00:00Z', 'admin', '127.0.0.1', 'login', 'admin', 'ok', '{}', '00', '00');
