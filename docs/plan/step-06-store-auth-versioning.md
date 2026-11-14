# Step 06：持久化、账户与配置版本（`xpxe-store`）

## 1. 目标

提供控制面需要的全部持久化能力：SQLite 存储与迁移、不可变配置版本链、审计哈希链、BMC 密码的信封加密、账户/会话/API Token/装机令牌。本 crate 对外暴露同步 API，控制面通过 `tokio::task::spawn_blocking` 调用，单写连接 + 多读连接（WAL）。

## 2. 选型

| 决策 | 理由 |
| :--- | :--- |
| SQLite（`rusqlite` + `bundled`） | 单文件、事务、WAL、在线备份（`VACUUM INTO`）；审计与历史需要按条件查询与分页；`bundled` 把 SQLite 静态编进二进制，运行期零系统依赖 |
| 迁移 | `migrations/NNNN_*.sql` 以 `include_str!` 编入，启动时按 `PRAGMA user_version` 顺序执行，每个迁移在事务里 |
| 时间 | 统一存 UTC RFC 3339；WebUI 按浏览器时区显示 |

## 3. 结构

```text
crates/xpxe-store/
├── migrations/0001_init.sql …
└── src/
    ├── lib.rs           Store::open(data_dir, master_key) / backup / integrity_check
    ├── config.rs        配置对象与版本链
    ├── nodes.rs         节点运行态、会话、事件（含幂等 EventId 表）
    ├── auth.rs          用户、会话、API Token、登录退避
    ├── tokens.rs        装机令牌生成、哈希、按种类解析
    ├── assets.rs        资产元数据
    ├── audit.rs         审计哈希链
    ├── crypto.rs        主密钥、信封加密
    └── observations.rs  bootnet 观测（按时间滚动保留 30 天）
```

表结构见 README 第 5 节。

## 4. 设计要点

### 4.1 配置版本链

- `save(kind, object_id, doc, author, comment) -> VersionRef`：在一个事务内校验（`ConfigDocument::validate`）、规范化、计算 `content_sha256`；与当前头版本哈希相同则直接返回头版本，否则写入 `version = head + 1` 并移动 `head_version`。乐观并发：调用方必须携带 `expected_head`，不一致返回 `Conflict { current_head }`，Web 提示“已被他人修改”并展示差异。
- `rollback(object_id, to_version, author)`：读取目标版本内容，作为新版本保存，`comment` 自动标注来源版本号。历史永不改写、永不删除。
- `diff(object_id, from, to)`：调用 `xpxe_domain::config::diff`。
- 会话固定：arm 时把 Profile、初始化计划、资产频道的版本号写入 `install_sessions.pinned`，装机过程中修改配置不影响正在进行的装机。

### 4.2 审计哈希链

`hash = SHA-256(prev_hash ‖ canonical_json(entry))`，首条 `prev_hash` 为 32 字节零。`verify()` 从头重算并返回第一处断链位置。审计只追加；`xpxe-control audit export --since` 导出 JSON Lines 供外部归档。

### 4.3 主密钥与信封加密

- 主密钥从 `XPXE_MASTER_KEY_FILE` 读取 32 字节（文件为原始字节或 base64），文件权限宽于 `0400`/`0440` 时警告；缺失或长度不对时拒绝启动。
- 每个 BMC 密码用 XChaCha20-Poly1305 加密，`AAD = bmc_object_id ‖ key_id`，防止密文在记录间挪用。`key_id` 为主密钥 SHA-256 的前 8 字节，库里登记当前 `key_id`；启动时主密钥与登记不符即拒绝启动，避免用错密钥。
- 轮换：`xpxe-control rotate-master-key --old-file A --new-file B` 在一个事务内重加密全部密文并更新 `key_id`。
- 数据卷里不出现任何密钥材料；数据库备份文件单独泄露时，BMC 密码仍不可读。

### 4.4 账户与会话

| 项 | 规则 |
| :--- | :--- |
| 口令哈希 | argon2id（m = 64 MiB，t = 3，p = 1），口令长度 12–256 |
| 初始管理员 | `users` 为空时读取 `XPXE_ADMIN_INITIAL_PASSWORD_FILE` 创建 `admin`，`must_change_password = true` |
| 会话 | 256 bit 随机 ID，Cookie 存原值（`HttpOnly; SameSite=Strict; Path=/`，管理端为 HTTPS 时追加 `Secure`），库里存 SHA-256；CSRF 令牌同理；空闲 30 min，绝对 12 h；改密后注销该用户其他会话 |
| 登录退避 | 按用户名与来源 IP 分别计数，连续失败第 5 次起按 2ⁿ 秒退避，上限 15 min；退避期间返回统一错误，不暴露用户是否存在 |
| API Token | 前缀 `xpxe_` + 256 bit，库存哈希；作用域 `read`、`operate`、`admin`；必须设置过期时间（≤ 365 天） |

### 4.5 装机令牌

```rust
pub enum TokenKind { Boot, Agent }
pub fn issue_session(node, channel, pinned, ttl) -> (SessionId, BootToken, AgentToken);
pub fn resolve(token: &str) -> Option<(SessionId, TokenKind)>;   // 常量时间比较哈希
pub fn consume_once(session, Doc::Preseed | Doc::Plan) -> Result<(), AlreadyServed>;
```

- 令牌为 32 字节 CSPRNG（`rand_core::OsRng`），base64url 无填充，库里只存 SHA-256。
- `resolve` 只接受未过期、未关闭会话的令牌；`Boot` 令牌只能访问 iPXE 脚本、内核、initrd、preseed、ISO、仓库公钥；`Agent` 令牌只能访问 agent、计划与事件端点。
- `consume_once` 用 `UPDATE … SET preseed_served_at = ? WHERE preseed_served_at IS NULL` 原子实现一次性。

## 5. 测试

| 测试 | 内容 |
| :--- | :--- |
| 迁移 | 空库到最新；每个迁移可在含样例数据的库上执行 |
| 版本链 | 相同内容不产生新版本；冲突检测；回滚生成新版本且历史不变；会话固定后修改配置不影响会话 |
| 审计 | 篡改任意一行（直接改 SQLite）后 `verify` 指出该行 |
| 加密 | 错误主密钥拒绝启动；密文挪到另一条记录解密失败；轮换后旧密钥失效、新密钥可读 |
| 认证 | argon2 参数；会话过期；退避时间序列；改密注销其他会话 |
| 令牌 | 种类越权访问被拒；过期与关闭会话被拒；`consume_once` 在 16 个并发调用下只有一个成功 |
| 崩溃安全 | 事务中途中断（测试里 drop 连接）后数据库一致 |

## 6. 验收

```bash
cargo test -p xpxe-store --all-targets --locked
cargo run -p xpxe-lab -- store-smoke --data /tmp/xpxe-data --master-key-file /tmp/mk   # 建库、写配置、回滚、审计校验、备份
test ! -e /tmp/xpxe-data/master.key && ! grep -rqa "$(base64 -w0 /tmp/mk)" /tmp/xpxe-data
scripts/gate.sh
```

验收标准：

- 数据目录中找不到主密钥文件或其内容。
- `store-smoke` 输出审计链校验通过、备份文件可独立打开。
- `scripts/gate.sh` 全绿。
