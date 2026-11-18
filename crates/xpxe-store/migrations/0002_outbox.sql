-- 发件箱：状态转换产生的副作用，与转换在同一事务写入，由控制面后台 worker 执行。
CREATE TABLE outbox (
    id         INTEGER PRIMARY KEY,
    node_id    INTEGER NOT NULL REFERENCES nodes (id),
    session_id TEXT,
    effect     TEXT NOT NULL,
    payload    TEXT NOT NULL,
    attempts   INTEGER NOT NULL DEFAULT 0,
    next_at    TEXT NOT NULL,
    created_at TEXT NOT NULL,
    done_at    TEXT,
    last_error TEXT
) STRICT;
CREATE INDEX outbox_due ON outbox (done_at, next_at);
