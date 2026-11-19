-- Step 14：手动 ISO 网络引导。ISO 文件本体按内容寻址存于 /data/assets/iso/<sha256>。
CREATE TABLE iso_images (
    id              INTEGER PRIMARY KEY,
    sha256          TEXT NOT NULL,
    size            INTEGER NOT NULL,
    label           TEXT NOT NULL,
    volume_id       TEXT NOT NULL,
    hybrid          INTEGER NOT NULL,
    detected_family TEXT NOT NULL,
    detection       TEXT NOT NULL,
    uploaded_by     TEXT NOT NULL,
    uploaded_at     TEXT NOT NULL,
    deleted_at      TEXT
) STRICT;
CREATE UNIQUE INDEX iso_images_live ON iso_images (sha256) WHERE deleted_at IS NULL;

-- 抽取产物（kernel / initrd / initrd_with_iso），同一 (iso, 种类, 路径, 嵌入名) 只生成一次。
CREATE TABLE iso_extracts (
    iso_sha256  TEXT NOT NULL,
    kind        TEXT NOT NULL,
    source_path TEXT NOT NULL,
    embed_name  TEXT NOT NULL DEFAULT '',
    sha256      TEXT NOT NULL,
    size        INTEGER NOT NULL,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (iso_sha256, kind, source_path, embed_name)
) STRICT;
CREATE INDEX iso_extracts_sha ON iso_extracts (sha256);

-- 进行中的分块上传（已收字节数以磁盘上的部分文件为准）。
CREATE TABLE iso_uploads (
    id              TEXT PRIMARY KEY,
    label           TEXT NOT NULL,
    size            INTEGER NOT NULL,
    expected_sha256 TEXT,
    created_by      TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;
