# Step 01：Workspace 骨架与质量门禁

## 1. 目标

建立全部 crate 的空骨架、锁定工具链、落地 Rust 门禁与 Web 门禁脚本，并在写业务代码之前确认计划使用的依赖能在门禁下零告警编译。本步结束后，后续每一步只往骨架里填内容，门禁本身保持不变。

## 2. 交付物

| 路径 | 内容 |
| :--- | :--- |
| `rust-toolchain.toml` | 按 [`rust-lint-gate.md`](../rust-lint-gate.md) 第三节锁定 `1.100.0`，`targets = ["x86_64-unknown-linux-musl"]` |
| `Cargo.toml` | 虚拟 workspace，`resolver = "3"`，`[workspace.package]` 全部元数据，`[workspace.lints]` 原样采用门禁文档第四节清单 |
| `clippy.toml`、`rustfmt.toml` | 原样采用门禁文档 |
| `scripts/gate.sh`、`scripts/lint-guard.py` | 原样采用门禁文档第五节 |
| `crates/*/` | 14 个 crate（见 README 第 4 节），每个都有 `Cargo.toml`（`[lints] workspace = true`）、带 `//!` 文档的 `lib.rs`/`main.rs`、`#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]` |
| `crates/xpxe-test-support/` | `ensure`、`ensure_eq`、`fixture_bytes(path)`、`TempDataDir` |
| `crates/xpxe-e2e/` | 空的 `tests/smoke/main.rs + cases.rs`，确认目录形式集成测试能被门禁接受 |
| `web/` | Vite + React 19 + TS 空壳，`package-lock.json` 提交 |
| `scripts/web-gate.sh` | Web 门禁骨架（Step 11 补全检查项，本步先接通命令链） |
| `deploy/`、`lab/`、`trust/` | 空目录与 `README.md` 说明用途 |
| `Cargo.lock` | 提交 |

## 3. 设计要点

### 3.1 crate 元数据

`[workspace.package]` 统一提供 `version`、`edition = "2024"`、`rust-version = "1.100"`、`license`、`description`、`repository`、`readme`、`keywords`、`categories`、`publish = false`，满足 `cargo_common_metadata`。每个 crate 的 `description` 用 crate 级覆盖写清职责。

### 3.2 依赖白名单与特性收敛

所有依赖写在 `[workspace.dependencies]`，成员 crate 只用 `foo.workspace = true`。`multiple_crate_versions` 的目标是依赖树完全压平（每个 crate 只有一个版本）。注意 clippy 依据 `cargo metadata` 的全平台解析结果判断重复，Windows 专用依赖（`windows-sys`）和过程宏的 `syn` 也计入；另外 Cargo 会把弱依赖特性（`ring?/alloc`）引用的可选依赖写进锁文件，所以 `rustls-webpki` 无论选哪种 crypto provider 都会把 `ring` 带进解析结果。依赖选型因此以 `ring 0.17` 的依赖代际（`getrandom 0.2`、`windows-sys 0.52`）为基准，统一关闭默认特性、按需开启：

| 依赖 | 用于 | 版本与特性约束 |
| :--- | :--- | :--- |
| `tokio` | 全部异步 | `~1.46`：`rt-multi-thread, net, time, fs, io-util, signal, sync`。1.47 起改用 `windows-sys 0.59+`，与 `ring` 冲突 |
| `axum`、`tower`、`tower-http` | 控制面 | `axum 0.8` 只开 `http1, tokio, json, query, form, matched-path, original-uri`；`tower-http 0.7` 只开 `trace, limit, set-header` |
| `hyper`、`hyper-util`、`http`、`http-body-util`、`bytes` | 资产下载、Redfish 的 HTTP 客户端，管理端 HTTPS 的连接层 | `hyper-util` 锁 `0.1.16`（之后的版本要求 `socket2 0.6`），只开 `tokio, client-legacy, http1` |
| `hyper-rustls`、`rustls`、`tokio-rustls`、`webpki-roots` | HTTPS 客户端、管理端可选 HTTPS、Redfish 证书固定 | 单一 crypto provider `ring`；根证书用内嵌的 `webpki-roots`（`FROM scratch` 镜像里没有系统证书库） |
| `ring` | 随机数（`SystemRandom`）、SHA-256/512、OpenPGP 签名中的 RSA PKCS#1 v1.5 与 Ed25519 验证 | `std, alloc` |
| `socket2` | `SO_BROADCAST`、`SO_BINDTODEVICE`、`IP_PKTINFO` | `~0.5.10`，`all` |
| `minijinja` | 模板 | `3.x`，关闭默认特性，只开 `serde`；模板 `include_str!` 编入 |
| `sha2`、`hex`、`base64` | 哈希与编码 | `sha2 0.11`、`base64 0.23` |
| `lzma-rs`、`flate2`（`rust_backend`） | `Packages.xz`/`.gz`、initrd 识别 | - |
| `rusqlite` | 存储 | `bundled`（静态编入 SQLite，构建期需要 `musl-tools`，运行期零依赖） |
| `argon2`、`chacha20poly1305`、`sha-crypt` | 口令、密钥封装、`$6$` 哈希 | `argon2 0.6`、`chacha20poly1305 0.11`、`sha-crypt 0.6`，都关闭 `getrandom` 特性，盐与 nonce 由 `ring::rand::SystemRandom` 生成 |
| `serde`、`serde_json` | 序列化 | `serde ~1.0.228`：之后的 `serde_derive` 改用 `syn 3`，而 `tokio-macros`（`axum` 的 `tokio` 特性引入）仍是 `syn 2` |
| `tracing`、`tracing-subscriber` | 日志（替代 `print_stdout`） | `fmt, json, env-filter, std`，不开 `ansi` |
| `time` | 时间戳 | `serde, formatting, parsing` |

以下能力在项目内自行实现，不引入第三方 crate：

| 能力 | 实现位置 | 不用现成 crate 的原因 |
| :--- | :--- | :--- |
| DHCPv4 报文编解码 | `xpxe-dhcp` | `dhcproto` 引入 `hickory-proto`、`rand 0.10` 与 `syn 2` 过程宏，依赖树无法与其余部分压平；特权进程 `xpxe-bootnet` 的解析面也应尽量小。BOOTP 定长头 + TLV 选项，约 400 行，配合抓包夹具与模糊输入测试 |
| ISO9660 + El Torito、FAT12/16 ESP 镜像 | `xpxe-image` | `fatfs 0.3` 依赖 `bitflags 1`；`hadris-iso`/`hadris-fat` 引入 `chrono` 时钟（Windows 下 `windows-core`）、`rand` 与 `syn 3` 派生宏。引导 ISO 结构固定（少量文件 + 一个 ESP 引导映像），自写写出器可完全控制布局 |
| OpenPGP 签名验证子集 | `xpxe-assets` | rPGP（`pgp`）自带两代 RustCrypto 依赖并存，无法压平；`sequoia-openpgp` 默认依赖 nettle（C 库）。Debian `InRelease` 只需要“v4 签名包 + RSA/Ed25519 公钥 + SHA-256/512”这一子集，用 `ring` 完成密码学部分 |
| UUIDv7 | `xpxe-store` | `uuid 1.x` 新版依赖 `getrandom 0.4`；UUIDv7 = 48 bit 毫秒时间戳 + 74 bit 随机数，十几行即可 |
| 测试用 TLS 证书 | 夹具 `tests/fixtures/tls/` | `rcgen` 引入 `thiserror 2`、`displaydoc`（`syn 3`）。测试证书一次性生成后入库，只用于测试 |

### 3.3 依赖预检

`crates/xpxe-lab` 暂时把上表全部依赖各调用一次最小 API（例如构造一个 `dhcproto` 报文、建一个内存 `fatfs` 卷），然后跑完整门禁。目的：

1. 确认第三方派生宏和宏展开不触发门禁（门禁不允许豁免，只能换写法或换依赖）。
2. 拿到 `clippy::multiple_crate_versions` 的真实结果，并把依赖树压平：出现重复版本时依次调整特性、统一上游版本、替换引入重复的依赖。只有在这些手段都无法消除时，才在门禁文档 [`rust-lint-gate.md`](../rust-lint-gate.md) 中把该 lint 从清单删除，同时写明重复的 crate、完整来源链、已尝试的手段与无法消除的原因，并附项目负责人的批准记录。任何情况下都不在代码、`Cargo.toml` 或 `clippy.toml` 中豁免该 lint（不使用 `allow`、`expect`、`allowed-duplicate-crates`）。
3. 确认 `x86_64-unknown-linux-musl` 目标能链接 `rusqlite(bundled)` 与 `ring`。

预检结束后 `xpxe-lab` 里的预检代码保留为 `xpxe-lab deps-check` 子命令，工具链升级时复用。

### 3.4 Web 门禁骨架

`scripts/web-gate.sh` 在 `web/` 下依次执行：`npm ci`（CI 预热依赖缓存）→ `npm run typecheck` → `npm run lint -- --max-warnings 0` → `npm run test` → `npm run build` → `node scripts/scan-dist.mjs`。本步各命令跑在空壳上，Step 11 按 [`webui-style-guide.md`](../webui-style-guide.md) 补齐规则。

## 4. 验收

```bash
# 工具链与门禁
rustc --version | grep -F "rustc 1.100.0 "
scripts/gate.sh

# 预检：目标三元组与依赖
cargo build --locked --target x86_64-unknown-linux-musl -p xpxe-lab
cargo run --locked -p xpxe-lab -- deps-check

# 门禁能拦截违规（逐条注入后必须失败，再还原）
printf '#[allow(dead_code)]\nfn x() {}\n' >> crates/xpxe-domain/src/lib.rs && ! scripts/gate.sh; git checkout crates/xpxe-domain/src/lib.rs
mkdir -p tests && touch tests/root.rs && ! scripts/gate.sh; rm -rf tests

# Web 骨架
scripts/web-gate.sh
```

验收标准：

- `scripts/gate.sh` 输出 `gate: 全部通过`，所有成员 crate 的 `[lints]` 只有 `workspace = true`。
- `deps-check` 成功，`multiple_crate_versions` 为零（`cargo tree -d --target x86_64-unknown-linux-musl -e normal,build` 无输出）；如无法压平，门禁文档已删除该项并附负责人批准的书面理由，且 `rg -n 'multiple_crate_versions|allowed-duplicate-crates' crates clippy.toml Cargo.toml` 无命中。
- 两条注入违规都会让门禁失败。
- `scripts/web-gate.sh` 在空壳上通过。
