# Step 07：资产管理（`xpxe-assets`）

## 1. 目标

镜像里不内置任何 iPXE 固件或 Debian 安装文件。管理员在 Web 上从官方源下载（或离线上传），系统记录来源与 SHA-256、做格式与签名校验、按版本保存；人工激活前可先在金丝雀节点上试装；激活与回滚都是配置版本操作。

## 2. 资产种类

| 种类 | 官方来源（默认） | 校验 | 用途 |
| :--- | :--- | :--- | :--- |
| `ipxe_undionly` | `https://boot.ipxe.org/undionly.kpxe` | HTTPS + 主机白名单 + 格式 + SHA-256 记录 | BIOS PXE（TFTP） |
| `ipxe_efi_x64` | `https://boot.ipxe.org/ipxe.efi` | 同上 + PE32+/Machine 0x8664/Subsystem EFI_APPLICATION | UEFI PXE（TFTP）、HTTP Boot、Redfish ISO |
| `ipxe_snponly_x64` | `https://boot.ipxe.org/snponly.efi` | 同上 | 部分网卡上替代 `ipxe.efi`（DHCP 规则可选） |
| `debian_netboot_amd64` | `<镜像>/dists/trixie/main/installer-amd64/current/images/netboot/debian-installer/amd64/{linux,initrd.gz}` | OpenPGP 签名链（4.2）+ ABI 提取 | 安装内核与 initrd |
| `debian_firmware_cpio` | `https://cdimage.debian.org/cdimage/firmware/trixie/current/firmware.cpio.gz` | `SHA256SUMS` + `SHA256SUMS.sign`（Debian CD 签名密钥） | 可选，为需要非自由固件的网卡追加 initrd |

允许的官方主机白名单：`boot.ipxe.org`、`github.com`（`/ipxe/ipxe/releases/`）与其下载重定向主机、`deb.debian.org`、`cdimage.debian.org`；管理员也可填写任意 HTTPS URL（如内网制品库），来源记为 `url`。`xpxe-agent` 随镜像构建，不属于可下载资产。

## 3. 生命周期

```text
staged ──校验通过──▶ verified ──金丝雀成功──▶ canary_passed
   │                    │                        │
   └──校验失败：删除文件，保留失败记录与原因      └──▶（激活：写入 asset_channels 新版本）
```

| 操作 | 规则 |
| :--- | :--- |
| 获取 | `POST /assets/fetch {kind, source: official \| url}`：后台任务流式下载到 `/data/assets/.tmp/<uuidv7>`，边下边算 SHA-256，超过种类上限（iPXE 4 MiB、ISO 16 MiB、initrd 256 MiB）立即中止；完成后 `rename` 到内容寻址路径 `/data/assets/<sha256>`；进度经 SSE 推送 |
| 上传 | 同一流程，来源记为 `upload`；Debian 资产需同时上传 `InRelease` 与 `SHA256SUMS`（或 `netboot.tar.gz` + 二者），离线完成同样的签名校验 |
| 记录 | `source_url`（重定向后的最终 URL）、HTTP `ETag`/`Last-Modified`、大小、SHA-256、操作者、iPXE 版本串（从二进制中提取 `iPXE x.y.z+ (g<commit>)`）、Debian 内核 ABI 与 InRelease 日期 |
| 金丝雀 | `POST /assets/{id}/canary {node_id}`：以“候选资产覆盖”arm 一台节点，会话固定使用候选资产；该会话到达 `Ready` 后资产标记 `canary_passed` 并记录节点与会话 |
| 激活 | `POST /assets/activate {kind, asset_id}`：仅 `admin`；资产须为 `canary_passed`，或附带“跳过金丝雀”的书面理由（写审计）；生成 `asset_channels` 新版本 |
| 回滚 | `POST /assets/rollback {kind}`：把 `asset_channels` 回滚到上一个版本（Step 6 版本链），一键完成 |
| 删除 | 只能删除未被任何频道版本头与进行中会话引用的资产 |

正在进行的会话固定自己的资产版本；bootnet 的应答指令里带固件 SHA-256，TFTP 文件名使用 `fw/<sha256 前 12 位>/<文件名>`，多个版本可同时服务。

## 4. 校验细节

### 4.1 iPXE

官方预编译 iPXE 不提供签名，信任来自“HTTPS + 主机白名单 + 人工激活 + 金丝雀”。Web 上明确展示这一点，并在新获取的 SHA-256 与当前激活版本不同时列出两者的版本串与日期，供人工判断。格式检查：

| 文件 | 检查 |
| :--- | :--- |
| `.efi` | `MZ` 头、`PE\0\0` 签名、Optional Header Magic `0x20B`、Machine `0x8664`、Subsystem `10` |
| `undionly.kpxe` | 大小 32 KiB–512 KiB，包含 `iPXE` 版本串 |

### 4.2 Debian netboot 签名链

1. 下载 `dists/trixie/InRelease`，用 `xpxe-assets` 内的 OpenPGP 验证子集（v4 签名包，RSA PKCS#1 v1.5 / Ed25519，SHA-256/512，密码学部分由 `ring` 完成）验证内联签名。信任根为 `trust/debian-archive-keyring.asc`（从 Debian `debian-archive-keyring` 包导出的公钥，构建时 `include_bytes!` 编入），签名密钥指纹必须在 `trust/fingerprints.txt` 中且未过期；两者变更都是单独提交，指纹与 Debian 官方公布值交叉核对。
2. 从 InRelease 的 `SHA256:` 段取 `main/installer-amd64/current/images/SHA256SUMS` 的哈希，下载并比对。
3. 从 `SHA256SUMS` 取 `./netboot/debian-installer/amd64/linux` 与 `initrd.gz` 的哈希，下载并比对。
4. 从 bzImage 头读取版本串（`HdrS` 魔数位于 0x202，`kernel_version` 指针位于 0x20E，指向 `0x200 + 指针值`），得到 ABI，例如 `6.12.x+deb13-amd64`。
5. InRelease 的 `Date` 早于 30 天时给出警告（镜像可能滞后）。

`debian_firmware_cpio` 用 `SHA256SUMS.sign`（分离签名）验证 `SHA256SUMS`，再比对文件哈希，信任根同样来自 `trust/`。

### 4.3 内核 ABI 与镜像源对齐

d-i 启动后从镜像源下载与自身内核 ABI 一致的模块 udeb；镜像源已更新到新的小版本而 netboot 仍停留在较早的小版本时，安装会在“未找到内核模块”处失败。因此：

- `check_alignment(netboot_asset, mirror) -> Alignment`：验证镜像源的 `dists/trixie/InRelease`，按其中哈希下载 `main/debian-installer/binary-amd64/Packages.xz`（`lzma-rs` 解压；无 `.xz` 时用 `.gz`），确认存在 `Package: kernel-image-<ABI>-di`。
- 结果按（资产，镜像）缓存 24 h；arm 前必须有有效的“已对齐”结论，否则拒绝 arm，错误信息给出 netboot ABI 与镜像中现有 ABI 列表，并提供“获取与镜像一致的 netboot”按钮。
- 内网镜像（如 Nexus 代理）同样适用；镜像无签名时（内网自建源）只能在 Profile 上显式标注“镜像不签名”，此时跳过签名但仍做 ABI 对齐。

### 4.4 离线模式

`XPXE_OFFLINE=true` 时禁止访问第 2 节的官方公网主机，只能上传；对 Profile 中配置的镜像源的对齐检查照常进行（内网镜像）。

## 5. 测试

| 测试 | 内容 |
| :--- | :--- |
| 签名链（离线夹具） | `tests/fixtures/debian/`：真实的 InRelease、SHA256SUMS、截断的 linux 头部；篡改任一字节、换签名密钥、过期密钥、SHA256SUMS 不在 InRelease 中，均失败 |
| ABI 提取 | 真实 bzImage 头部夹具；非 bzImage、指针越界 |
| 对齐 | 夹具 `Packages.xz` 含/不含目标 ABI |
| 格式检查 | 真实 iPXE 二进制（`legacy/ipxe.efi`、`legacy/undionly.kpxe` 作为夹具）通过；HTML 404 页面、ARM64 EFI、截断文件被拒 |
| 下载流程 | 测试内起 HTTPS 服务（`tests/fixtures/tls/` 中的测试 CA 与服务端证书 + 自定义根）：重定向白名单、超大文件中止、断流、ETag 记录 |
| 生命周期 | 未 `canary_passed` 激活被拒；跳过金丝雀需理由并写审计；回滚；被引用资产不可删除 |

## 6. 验收

```bash
cargo test -p xpxe-assets --all-targets --locked
# 联网冒烟（CI 定时任务与实验室执行，不在 gate.sh 内）
cargo run -p xpxe-lab -- assets-smoke --data /tmp/xpxe-data \
  --fetch ipxe_efi_x64 --fetch ipxe_undionly \
  --fetch debian_netboot_amd64 --mirror http://deb.debian.org/debian
cargo run -p xpxe-lab -- assets-align --data /tmp/xpxe-data --mirror http://192.168.2.5:8081/repository/debian
scripts/gate.sh
```

验收标准：

- 联网冒烟拿到 3 个资产，均为 `verified`，Debian 资产显示签名密钥指纹与 ABI。
- 对 `deb.debian.org` 的对齐结论为“已对齐”；人为指定一个旧 netboot 夹具时结论为“未对齐”并列出镜像现有 ABI。
- `scripts/gate.sh` 全绿。

## 7. 实现记录

实现位于 `crates/xpxe-assets`（`armor`、`packet`、`pubkey`、`signature`、`trust`、`release`、`kernel`、`ipxe`、`align`、`fetch`、`kind`、`catalog`、`ingest`），实验室命令为 `xpxe-lab assets-smoke` / `assets-align`，联网冒烟在 `.github/workflows/assets-smoke.yml`（每周定时 + 手动）。

与上文的偏差：

| 项 | 计划 | 实现 | 原因 |
| :--- | :--- | :--- | :--- |
| iPXE EFI 官方地址 | `https://boot.ipxe.org/ipxe.efi`、`/snponly.efi` | `https://boot.ipxe.org/x86_64-efi/ipxe.efi`、`/x86_64-efi/snponly.efi`；`undionly.kpxe` 仍在根路径 | iPXE 2.0 起 boot.ipxe.org 按架构分目录，根路径 EFI 文件返回 404（2026-11-18 核实） |
| iPXE 版本串 | 从二进制提取 `iPXE x.y.z+ (g<commit>)` | 只在二进制含明文版本串时记录，否则为空 | 官方预编译文件（EFI 与 kpxe）均为压缩映像，不含明文版本串 |
| `undionly.kpxe` 检查 | 大小 32–512 KiB + 含 `iPXE` 版本串 | 大小 32–512 KiB + 以 iPXE PXE 前缀 `ljmp $0x07c0, …`（`EA xx xx C0 07`）开头 | 同上：压缩映像中没有 `iPXE` 字样 |
| `.efi` 截断检测 | 头部字段 | 头部字段 + 节表每个节的原始数据必须完整在文件内 | 只截断尾部的文件头部字段仍然完整 |
| 固件大小上限 | 未规定 | `firmware.cpio.gz` 1 GiB | trixie 的固件包约 473 MiB |
| 临时文件名 | `/data/assets/.tmp/<uuidv7>` | `/data/assets/.tmp/<128 位随机十六进制>` | 不引入 uuid 依赖，作用相同 |
| netboot 资产 | 内核与 initrd | 一个资产 = 内容寻址的清单文件（`linux`、`initrd.gz` 的哈希/大小与 ABI），两个成员文件各自内容寻址；`verify.files` 记录成员 | 资产表一行一个 SHA-256；删除时成员文件在无其他资产引用时一并删除 |
| 信任根密钥范围 | `debian-archive-keyring` 导出 | trixie / bookworm 的归档自动签名、安全归档自动签名、稳定版发布密钥（主钥 + 签名子钥分别列入白名单）及 Debian CD 签名密钥 | 当前 trixie `InRelease` 同时由 bookworm 与 trixie 自动签名钥签名；固件 `SHA256SUMS.sign` 由 CD 密钥签名 |
| 子钥绑定签名 | — | 不校验子钥绑定签名，改为把签名子钥指纹直接列入白名单 | 白名单本身就是逐把核对过的信任声明 |
| 对齐结果缓存 24 h、arm 前拒绝 | 本步 | 本步提供 `Ingest::alignment` 返回带 `checked_at` 的报告；缓存与 arm 拦截在 Step 10 控制面实现 | 缓存与 arm 属于控制面状态 |
| 校验失败记录 | 保留失败记录与原因 | 写入审计（`asset.verify_failed`，含错误码与原因），资产表不新增失败阶段 | 资产表阶段约束为 staged/verified/canary_passed/retired |
| 金丝雀 | 会话固定候选资产 | `Pinned.canary_assets`（serde 默认空）记录候选资产；到达 `Ready` 后由控制面调用 `Catalog::mark_canary_passed` | arm 流程在 Step 10 |
| 离线模式 | 禁止官方公网主机 | 官方源、自定义 URL、镜像三类来源都拒绝白名单内主机 | 防止通过“自定义 URL / 镜像”绕过 |
| 内网镜像验收 | `assets-align --mirror http://192.168.2.5:8081/repository/debian` | 本机实验环境无法访问该 Nexus（连接超时），以 `deb.debian.org` 完成验收 | 环境限制；命令本身对任意镜像通用 |

验收记录（2026-11-18，box 联网）：`assets-smoke` 获取 `ipxe_efi_x64`、`ipxe_undionly`、`ipxe_snponly_x64`、`debian_netboot_amd64`（ABI `6.12.107+deb13-amd64`，签名子钥 `4CB50190…2643E131`）、`debian_firmware_cpio`（CD 密钥 `DF9B9C49…6294BE9B`），全部 `verified`，审计链 6 条完整；`assets-align` 对 `deb.debian.org` 为“已对齐”，旧 netboot 夹具（`6.12.38+deb13-amd64`）为“未对齐”并列出 `6.12.107+deb13-amd64`、`6.12.94+deb13-amd64`。
