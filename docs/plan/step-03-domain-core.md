# Step 03：领域核心（`xpxe-domain`）

## 1. 目标

用纯函数实现系统的“规则大脑”：受约束的值类型、节点状态机与超时、DHCP 与 iPXE 层的引导决策、配置版本与差异。本 crate 不做 I/O、不读时钟（时间由调用方传入）、不依赖 tokio。

## 2. 模块

```text
crates/xpxe-domain/src/
├── lib.rs
├── value.rs        值类型：MacAddr、Hostname、Fqdn、Ipv4Cidr、UnixUser、PackageName、SshPublicKey、TimeZone、Locale、HttpUrl、SafeText
├── value/tests.rs
├── arch.rs         ClientArch 与 Option 93 映射
├── node.rs         NodeSpec、BootChannel、NodeNetwork、FirmwareMode
├── state.rs        NodeState、NodeEvent、Transition、Effect、Ttl
├── state/tests.rs
├── decision.rs     DhcpRequestView → DhcpAnswer；iPXE 入口决策
├── decision/tests.rs
├── config.rs       ConfigKind、ConfigDocument trait、规范化 JSON、结构化差异、Secret<T>
└── config/tests.rs
```

模块布局遵循 `foo.rs + foo/`，禁止 `mod.rs`；单元测试只写 `#[cfg(test)] mod tests;`。

## 3. 设计要点

### 3.1 值类型（杜绝注入的第一道防线）

每个对外输入都先解析成新类型，构造函数是唯一入口，内部不变量保证后续模板渲染安全：

| 类型 | 规则 |
| :--- | :--- |
| `MacAddr` | 6 字节；接受 `aa:bb:..`、`aa-bb-..`、`aabb..`；输出 `hexhyp` 与冒号两种格式 |
| `Hostname` | RFC 1123 单标签，1–63，小写 |
| `UnixUser` | `^[a-z_][a-z0-9_-]{0,31}$`，拒绝系统保留名（`root` 除外的 UID < 1000 账户名单） |
| `PackageName` | Debian Policy：`^[a-z0-9][a-z0-9+.-]+$` |
| `SshPublicKey` | 类型白名单（`ssh-ed25519`、`ecdsa-sha2-nistp256/384/521`、`ssh-rsa` ≥ 3072 bit、`sk-*`），base64 可解码且内嵌类型一致，注释仅可打印 ASCII，无换行 |
| `TimeZone` | 与 `data/zones.txt`（IANA 区名清单，随仓库提交）匹配 |
| `Locale` | 与 `data/locales.txt`（Debian `SUPPORTED` 清单）匹配 |
| `HttpUrl` | 仅 `http`/`https`，必须有主机，不含空白与控制字符 |
| `SafeText` | 单行、可打印 UTF-8、长度上限，任何控制字符直接拒绝 |

所有类型实现 `serde`，反序列化走同一个构造函数，数据库里读出的脏数据同样被拒绝。

`NodeSpec` 的跨字段约束：`FirmwareMode` 取 `Uefi` 或 `Bios`；`BootChannel::Redfish` 只接受 `FirmwareMode::Uefi`，`Bios` 节点只能使用 `BootChannel::Lan`（局域网 PXE）。违反时返回字段级错误 `channel: redfish_requires_uefi`，Web 在选择 BIOS 时禁用 Redfish 通道选项并说明原因。

### 3.2 客户端架构

```rust
pub enum ClientArch { BiosX86, UefiX64, UefiX64Http, Unsupported(u16) }
// Option 93：0 → BiosX86；7、9 → UefiX64；16 → UefiX64Http；6/10/11/15/19 等 → Unsupported
```

### 3.3 状态机

```rust
pub fn apply(current: &NodeRuntime, event: &EventEnvelope, now: Timestamp, ttl: &Ttl)
    -> Result<Transition, TransitionError>;

pub struct EventEnvelope { id: EventId, session: Option<SessionId>, source: EventSource, event: NodeEvent }
pub struct Transition { to: NodeState, deadline: Option<Timestamp>, effects: Vec<Effect> }
pub enum Effect { OpenSession, CloseSession(Outcome), PublishDirectives, EjectMedia, Audit(AuditKind) }
```

规则：

1. 转换表即 README 3.3 节的状态图；表外组合返回 `TransitionError::NotAllowed { from, event }`，不改变任何状态。
2. 来自安装器与 agent 的事件必须带 `session`，且等于当前会话，否则 `StaleSession`（防止上一次装机的迟到回调影响本次）。
3. `EventId` 由发送方生成；同一 `EventId` 重复提交返回首次结果（幂等），agent 可以放心重试。
4. 超时：进入 `Armed`/`Installing`/`Installed`/`Configuring` 时计算 `deadline = now + ttl`。定时器投递 `DeadlineExpired { state, since }`，只有 `state` 与 `state_since` 都匹配当前值才生效，晚到的定时器事件被丢弃。
5. `Ready → Armed` 的 `Rearm` 事件必须携带与节点主机名一致的确认串（Web 上由用户输入），领域层校验。
6. `Effect` 由控制面执行；领域层只描述“要做什么”。

### 3.4 引导决策

DHCP 层（bootnet 调用）：

```rust
pub fn answer(req: &DhcpRequestView, table: &DirectiveTable, rules: &DhcpRules, now: Timestamp) -> DhcpAnswer;

pub enum DhcpAnswer {
    Ignore(IgnoreReason),          // NotPxe / Relayed / NotArmed / Expired / UnsupportedArch / HttpBootDisabled
    Offer(BootOffer),              // 交给 xpxe-dhcp 编码
}
pub enum BootTarget { Tftp(FirmwareName), Http(HttpUrl) }
```

判定顺序（任何一步不满足即 `Ignore`，原因写入观测）：

1. `giaddr ≠ 0` → `Relayed`。
2. Opt60 不以 `PXEClient` 或 `HTTPClient` 开头 → `NotPxe`。
3. MAC 不在指令表，或指令已过期 → `NotArmed` / `Expired`。
4. Opt77 = `iPXE` 或携带 Opt175 → `Http(<boot_base>/p/{token}/boot.ipxe)`。
5. `BiosX86` → `Tftp(undionly.kpxe)`；`UefiX64` → `Tftp(ipxe.efi | snponly.efi)`（按 `DhcpRules`）；`UefiX64Http` 且 Opt60 以 `HTTPClient` 开头 → `Http(<boot_base>/fw/x64/ipxe.efi)` 并要求应答携带 Opt60=`HTTPClient`。
6. 其他架构 → `UnsupportedArch`。

iPXE 入口（控制面 `/boot/mac/{mac}` 调用）：`fn ipxe_entry(mac, runtime) -> IpxeEntry`，只有 `Armed` 返回 `Chain(session)`，其余返回 `Exit(reason)`，由渲染器输出 `exit` 脚本。

### 3.5 配置版本

```rust
pub enum ConfigKind { Profile, InitPlan, BootNetwork, DhcpRules, Timeouts, AssetChannels, NodeSpec, Bmc }
pub trait ConfigDocument: Serialize + DeserializeOwned {
    const KIND: ConfigKind;
    const SCHEMA: u32;
    fn validate(&self) -> Result<(), Vec<FieldError>>;   // FieldError { path, code, message }
}
pub fn canonical_json(doc: &impl ConfigDocument) -> Result<CanonicalJson, ConfigError>;
pub fn diff(a: &CanonicalJson, b: &CanonicalJson) -> Vec<Change>;   // Change { path, before, after }
```

- 规范化：键排序、无多余空白、数字统一格式，`content_sha256` 基于规范化结果计算，相同内容的重复保存不产生新版本。
- `Secret<T>` 字段序列化为 `{"secret_ref": "...", "fingerprint": "<前 8 位>"}`；差异里只显示“已更改”，永不输出明文。
- `FieldError.path` 使用 JSON Pointer，WebUI 直接映射到表单字段。
- `SCHEMA` 升级时提供 `migrate(from, value)`，迁移本身有单元测试。

## 4. 测试

| 文件 | 内容 |
| :--- | :--- |
| `state/tests.rs` | 7 个状态 × 11 个事件全组合表驱动：期望的目标状态或错误；会话不匹配；`EventId` 幂等；迟到的 `DeadlineExpired`；`Rearm` 确认串 |
| `decision/tests.rs` | 用 Step 2 抓到的真实报文（经 `xpxe-dhcp` 解析出的 `DhcpRequestView` JSON 夹具）逐个断言应答；中继、非 PXE、过期、ARM64、HTTP Boot 关闭 |
| `value/tests.rs` | 每个类型的合法/非法样例，重点覆盖换行、`$()`、反引号、`;`、NUL、超长 |
| `config/tests.rs` | 规范化稳定性、差异路径、`Secret` 不泄露、迁移 |

## 5. 验收

```bash
cargo test -p xpxe-domain --all-targets --locked
cargo test -p xpxe-domain --doc --locked
! cargo tree -p xpxe-domain -e normal | grep -qE 'tokio|hyper|rusqlite'   # 领域层无 I/O 依赖
scripts/gate.sh
```

验收标准：

- 77 个状态×事件组合全部有显式断言。
- 所有 Step 2 夹具都有对应的决策断言。
- `xpxe-domain` 的正常依赖中不出现 tokio、hyper、rusqlite。
- `scripts/gate.sh` 全绿。
