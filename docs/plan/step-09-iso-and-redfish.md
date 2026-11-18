# Step 09：Redfish 引导 ISO 与 Redfish 客户端（`xpxe-image`、`xpxe-redfish`）

## 1. 目标

Redfish 通道只走一条路径，且只支持 UEFI 固件：每次装机会话生成一个约 1–2 MB 的引导 ISO，里面是官方 `ipxe.efi` 加一段会话专属的 `autoexec.ipxe`，BMC 以 HTTP 方式挂载后一次性从虚拟光驱以 UEFI 模式启动，iPXE 再通过 HTTP 链到本次会话的安装脚本。BIOS 固件的服务器不使用 Redfish 通道，统一走局域网 PXE（Step 3 `NodeSpec` 约束在登记时就拒绝“BIOS + Redfish”）。ISO 全部在 Rust 里生成（ISO9660 + El Torito + FAT），不调用 xorriso、mtools、grub。

## 2. 引导 ISO

### 2.1 结构

```text
boot.iso（ISO9660，卷标 XPXE-<会话前 8 位>）
└── esp.img                       FAT（`xpxe-image` 在内存中新建）：
                                    EFI/BOOT/BOOTX64.EFI   当前会话固定的 ipxe.efi
                                    EFI/BOOT/autoexec.ipxe 会话脚本
                                  El Torito 引导目录只有一项：平台 0xEF（UEFI）、no-emulation，指向 esp.img
```

| 固件 | 引导方式 | 依据 |
| :--- | :--- | :--- |
| UEFI x64 | 固件从 `esp.img` 加载 `\EFI\BOOT\BOOTX64.EFI`；iPXE 从自身所在目录读取 `autoexec.ipxe` | iPXE `efi_autoexec`：先读加载目录，再读文件系统根目录（Step 2 H5） |

- 固件使用 `ipxe_efi_x64` 资产在本次会话中固定的版本，与 LAN 路径同一份文件。
- 生成是确定性的：所有时间戳取会话创建时间，同一会话重复生成得到相同字节；结果缓存在 `/data/cache/iso/<session>.iso`，会话关闭即删除。
- ISO 由 `xpxe-image` 的 ISO9660 + El Torito 写出器生成（Step 2 H13 验证）；FAT 由 `xpxe-image` 的 FAT 写出器在内存中格式化（按大小自动选择 FAT12/16）。

### 2.2 `autoexec.ipxe`

```ipxe
#!ipxe
set mac {{ boot_mac }}
set idx:int32 0
:find
isset ${net${idx}/mac} || goto nonic
iseq ${net${idx}/mac} ${mac} && goto found ||
inc idx && goto find
:found
{% if static %}set net${idx}/ip {{ ip }}
set net${idx}/netmask {{ netmask }}
set net${idx}/gateway {{ gateway }}
set dns {{ dns }}
ifopen net${idx} || goto failed
{% else %}dhcp net${idx} || dhcp net${idx} || goto failed
{% endif %}chain {{ boot_base }}/p/{{ boot_token }}/boot.ipxe || goto failed
:nonic
echo x-pxe: no interface with MAC ${mac}
:failed
echo x-pxe: network boot failed, returning to firmware in 10 s
sleep 10
exit
```

按节点规格中的 `boot_mac` 选网卡，多网卡服务器不会用错口；静态地址时 iPXE 自己配置网络，随后的内核参数（Step 4）让 d-i 也使用同一静态地址。

## 3. Redfish 客户端

### 3.1 TLS 与证书固定

| 阶段 | 行为 |
| :--- | :--- |
| `probe` | 接受任意证书完成握手，只请求无需认证的 `GET /redfish/v1`；返回证书 SHA-256、主题、颁发者、有效期与 `Vendor`/`RedfishVersion`。探测阶段不发送任何凭据 |
| `trust` | 管理员在 Web 上核对指纹后确认，指纹写入 `bmc` 配置新版本 |
| 正常调用 | 自定义 `rustls::client::danger::ServerCertVerifier`：只接受终端证书 DER 的 SHA-256 等于已固定值，握手签名仍按 rustls 标准校验；不校验 CA 与主机名（BMC 普遍为自签、以 IP 访问） |
| 指纹变化 | 调用失败，节点详情与审计中记录“BMC 证书已变化”，需重新 `probe` + `trust` |

实现不使用 `unsafe`，`danger` 只是 rustls 的模块名。

### 3.2 会话与资源发现

1. 优先 `POST /redfish/v1/SessionService/Sessions` 取 `X-Auth-Token`，结束时 `DELETE` 会话；服务不支持时使用 Basic 认证。
2. `Systems` 集合：只有一个成员时直接使用；多个成员时使用节点规格中的 `system_id`。
3. VirtualMedia 位置：优先 `ComputerSystem.VirtualMedia`，否则遍历 `Managers[*].VirtualMedia`；选择 `MediaTypes` 含 `CD` 或 `DVD` 的成员。
4. 读取 `Boot.BootSourceOverrideTarget@Redfish.AllowableValues` 确认支持 `Cd`。

### 3.3 厂商画像

```rust
pub struct VendorProfile {
    pub vmedia: VmediaLocation,          // System | Manager | Auto
    pub insert_fields: InsertFields,     // 是否携带 TransferMethod/TransferProtocolType/WriteProtected
    pub eject_before_insert: bool,
    pub boot_override: BootOverrideStyle,// Standard（PATCH Boot）| 其他厂商方式（按需扩展）
    pub reset: ResetPolicy,              // 已开机 ForceRestart / GracefulRestart；已关机 On
    pub task_polling: bool,              // 202 + TaskMonitor
}
```

内置画像：`Generic`（DMTF 标准，sushy-tools）、`DellIdrac9`、`HpeIlo5`/`HpeIlo6`、`SupermicroX12`、`LenovoXcc`。按 `ServiceRoot.Vendor` 自动选择，节点规格可手动覆盖。真机未验证的画像在 Web 上标注“未验证”。

### 3.4 部署流程（`POST /nodes/{id}/redfish/deploy`）

| # | 动作 | 失败处理 |
| :--- | :--- | :--- |
| 1 | 前置：节点 `Armed`（未 arm 时同一请求先 arm）、BMC 已固定指纹、Netboot 对齐通过、ISO 生成成功 | 返回字段级错误，不触碰 BMC |
| 2 | 已插入介质时 `EjectMedia` | 记录并继续 |
| 3 | `InsertMedia { Image: <boot_base>/p/<boot_token>/boot.iso, Inserted: true, WriteProtected: true, TransferMethod: Stream, TransferProtocolType: HTTP }`（字段按画像裁剪） | 解析 `@Message.ExtendedInfo` 展示 |
| 4 | 轮询直至 `Inserted = true`（≤ 60 s） | 超时则失败 |
| 5 | `PATCH Boot { BootSourceOverrideTarget: Cd, BootSourceOverrideEnabled: Once, BootSourceOverrideMode: UEFI }`（BMC 不接受 `BootSourceOverrideMode` 时省略该字段，并要求 BMC 报告的当前模式为 UEFI） | 当前模式为 Legacy 时失败并提示“该服务器需切换到 UEFI 或改用局域网 PXE” |
| 6 | 按 `PowerState` 与画像执行 `Reset` | - |
| 7 | 收到 `installer.started` 后状态机产生 `Effect::EjectMedia`，自动弹出 | 弹出失败只告警，不影响装机 |

每一步都写节点事件与审计。部署失败时节点保持 `Armed`，可重试或 disarm。另提供 `redfish/eject` 与 `redfish/power { On \| ForceOff \| ForceRestart }`（需二次确认）。

## 4. 测试

| 测试 | 内容 |
| :--- | :--- |
| ISO 结构 | 生成后用 `xpxe-image` 的读回解析器（独立于写出器实现）读回：卷标、文件树、El Torito 只有一项且平台为 0xEF、`esp.img` 内两份文件与字节一致 |
| 确定性 | 同一输入两次生成字节一致 |
| `autoexec.ipxe` 快照 | DHCP / 静态两种 |
| 证书固定 | 测试内 HTTPS 服务（`tests/fixtures/tls/` 测试证书）：正确指纹通过、错误指纹失败、`probe` 请求不含 `Authorization` 头 |
| Redfish 流程 | 测试内模拟 BMC（axum），按每个厂商画像模拟资源布局与差异：VirtualMedia 在 System/Manager 下、需先弹出、202 + Task、401、`ExtendedInfo` 错误、`Inserted` 延迟变为 true |
| DMTF Mockup | 用 DMTF 公开的 Redfish Mockup（`public-rackmount1` 等）做资源发现回归 |

## 5. 验收

```bash
cargo test -p xpxe-image -p xpxe-redfish --all-targets --locked
cargo run -p xpxe-lab -- iso --session-fixture lab/sessions/redfish-uefi.json --out /tmp/boot.iso
lab/run.sh R1 R2 --iso /tmp/boot.iso   # sushy-tools + OVMF（R2 为静态地址）：挂载、一次性 CD 启动、iPXE 链到 HTTP 脚本、进入 d-i
lab/run.sh redfish-bios-reject      # 登记“BIOS + Redfish”节点返回 redfish_requires_uefi
lab/run.sh redfish-pin-change       # 更换 sushy-tools 证书后部署必须失败并提示证书变化
scripts/gate.sh
```

验收标准：

- R1、R2 通过（矩阵编号见 Step 2 第 5 节），`redfish-bios-reject` 通过。
- `boot.iso` 小于 5 MB，同一会话重复下载内容一致。
- 实验室 HTTP 访问日志中，ISO 之后依次出现 `boot.ipxe`、`linux`、`initrd.gz`、`preseed.cfg` 请求。
- `scripts/gate.sh` 全绿。

## 6. 实现偏差（落地记录）

| 计划 | 实现 | 理由 |
| :--- | :--- | :--- |
| ISO 内文件名 `esp.img` | `ESP.IMG` | ISO9660 一级文件名只允许大写字母、数字与下划线 |
| 卷标 `XPXE-<会话前 8 位>` | 取会话 ID 前 8 个**字母数字**并大写（跳过 `-`），不足 8 个拒绝 | UUID 形式的会话 ID 前 8 位可能含连字符 |
| `autoexec.ipxe` 模板 | 位于 `xpxe-render::redfish_boot`（模板 `templates/autoexec.ipxe.j2`）；网关、DNS 为空时省略对应 `set` 行；渲染结果补回末尾换行 | 渲染统一放在 `xpxe-render`；`xpxe-image` 保持零依赖 |
| ISO 缓存 `/data/cache/iso/<session>.iso`、会话关闭即删 | Step 9 只提供确定性生成函数 `xpxe_image::session::build_boot_iso`；缓存与删除由控制面在 Step 10 实现 | 会话生命周期在控制面 |
| 读回解析器 | `xpxe_image::readback`（ISO9660 PVD/根目录、El Torito 验证项校验和与各节、FAT12/16 BPB/簇链/长文件名），对外公开，损坏输入返回 `ImageError::Malformed` | 同时供实验室/控制面自检 |
| 模拟 BMC 用 axum | 测试内手写 HTTP/1.1 + tokio-rustls 服务（`tests/redfish/server.rs`、`bmc.rs`），差异用 `Quirk` 枚举组合 | 不为测试开启 hyper server 相关特性；`max-struct-bools` 限制 |
| 部署第 1 步前置检查（Armed、指纹已固定、Netboot 对齐、ISO 生成）、节点事件/审计、第 7 步 `Effect::EjectMedia` | 由控制面在 Step 10 编排；`xpxe-redfish` 提供第 2–6 步 `Session::deploy`（逐步产出 `DeployEvent`）、`Session::eject`、`Session::power` | 控制面在 Step 10 落地 |
| `InsertMedia` 字段按画像裁剪 | Generic/HPE：`Image`+`Inserted`+`WriteProtected`；Dell/Supermicro：另加 `TransferProtocolType: HTTP`；Lenovo：另加 `TransferMethod: Stream` | sushy-tools 2.2 与 iLO 不接受多余字段；非 Generic 画像均标注“未验证” |
| 厂商画像中的布尔开关 | `InsertFields` 用 `TransferFields` 枚举（Omit/ProtocolOnly/MethodAndProtocol）+ `write_protected` | 门禁 `max-struct-bools = 2` |
| `HpeIlo6` 自动识别 | Vendor 含 HPE 时自动选 `HpeIlo5`，`HpeIlo6` 需节点规格手动指定 | 只凭 `ServiceRoot.Vendor` 无法区分代际 |
| 202 + Task | 画像 `task_polling` 为真且有 `Location` 时轮询到 `TaskState` 结束（Exception/Killed/Cancelled 为失败），上限 `Timing::task`（默认 120 s） | 计划未给任务上限 |
| 证书变化识别 | 固定模式握手失败时比较握手中看到的终端证书指纹，与固定值不同则报 `cert_changed`（消息含新旧指纹）；握手失败前不发送任何请求 | — |
| `probe` 证书主题/有效期 | 自写极简 DER 解析（`x509.rs`），只取主题、颁发者、有效期，解析失败时为空 | 避免新增 X.509 依赖与重复依赖 |
| 会话 | sushy-tools 2.2 无 `SessionService`，实验室走 Basic 回落；令牌会话由模拟 BMC 覆盖 | — |
| 验收命令 `lab/run.sh R1 R2 --iso /tmp/boot.iso` | `lab/run.sh --iso /tmp/boot.iso R1` 与 `lab/run.sh R2` 分开运行；`--iso` 只覆盖 R1（R2 为静态地址，使用 `lab/sessions/redfish-uefi-static.json` 生成的 ISO）；选项须写在场景名之前 | R1/R2 的服务模式不同（proxy/noproxy）；两份会话脚本不同 |
| 会话脚本 `p/<token>/boot.ipxe` | 实验室由 `lab/lib/redfish.sh` 写入：R1 复制通用 `boot.ipxe`，R2 写静态内核参数脚本 | 正式渲染在控制面（Step 10） |
| `redfish-bios-reject` | `xpxe-lab node-check --spec lab/nodes/redfish-bios.json` 走 `NodeSpec::new` 约束（控制面登记 API 在 Step 10） | — |

### 实验室验收记录（2026-11-18 Asia/Shanghai）

- `lab/run.sh redfish-bios-reject` PASS：`/channel redfish_requires_uefi`；UEFI + Redfish 登记通过。
- `lab/run.sh redfish-pin-change` PASS：更换 sushy-tools 证书后用旧指纹部署失败（`cert_changed`，含新旧指纹），sushy 未收到 InsertMedia/PATCH；重新探测得到新指纹后调用成功；结束后恢复原证书。
- `lab/run.sh --iso /tmp/boot.iso R1` PASS（38 s）：ISO 1 077 248 字节；重复生成与两次下载字节一致；`xpxe-lab redfish deploy`（Generic，Basic 回落）insert→inserted→boot_override→reset；OVMF 一次性 CD 启动 → `p/lab-redfish-r1/boot.ipxe` → d-i；访问日志依次 boot.iso → boot.ipxe → linux → initrd.gz → preseed.cfg。
- `lab/run.sh R2` PASS（20 s）：同上，静态地址 10.77.0.60；H6：iPXE 与 d-i 均未发 DHCP。
