# Step 08：引导网络——ProxyDHCP 与只读 TFTP（`xpxe-dhcp`、`xpxe-tftp`、`xpxe-bootnet`）

## 1. 目标

用 Rust 实现特权引导网络进程：ProxyDHCP（只给引导信息、不分配地址）、只读 TFTP、现网 DHCP 探测。它只应答处于 `Armed` 的 MAC，应答内容完全来自控制面推送的指令快照，不读任何配置文件，与控制面失联即停止应答。整个系统不使用 dnsmasq。

## 2. 交付物

| 路径 | 内容 |
| :--- | :--- |
| `crates/xpxe-dhcp` | 报文解析为 `DhcpRequestView`、`DhcpAnswer` 编码为报文、`DhcpTransport` trait、socket 实现 |
| `crates/xpxe-tftp` | 只读 TFTP 协议状态机（纯逻辑）+ tokio 传输 |
| `crates/xpxe-bootnet` | 二进制：指令同步、固件缓存、DHCP/TFTP 服务、现网 DHCP 探测、观测上报、健康状态 |
| `crates/xpxe-render/src/dhcp_snippets.rs` | “复用现有 DHCP”模式下给 ISC Kea、ISC dhcpd、Windows DHCP Server、MikroTik RouterOS 的配置片段 |
| `docs/lab/compat-matrix.md` | 在 Step 2 基础上补全 |

## 3. 运行模式（`boot_network` 配置，Web 可改、版本化）

| 模式 | bootnet 行为 |
| :--- | :--- |
| `ProxyDhcp { iface }` | 监听 67/udp、4011/udp、69/udp；对 `Armed` MAC 发 Proxy OFFER/ACK；TFTP 服务固件 |
| `ExternalDhcp { iface }` | 只监听 69/udp；DHCP 由现网服务器负责，Web 显示配置片段与探测结论 |
| `RedfishOnly` | 关闭全部监听，释放 67/69/4011 |

`iface` 只能从 `XPXE_BOOTNET_ALLOWED_IFACES` 中选择。模式切换由新指令快照触发，bootnet 先关闭旧 socket 再打开新 socket，切换过程写观测事件。

## 4. ProxyDHCP

### 4.1 收包与判定

- socket：`0.0.0.0:67` 与 `0.0.0.0:4011`，`SO_REUSEADDR`、`SO_BROADCAST`、`SO_BINDTODEVICE(iface)`、`IP_PKTINFO`（取得收包网卡与目的地址）。
- 解析（`xpxe-dhcp` 自研编解码）：`op`、`xid`、`flags`、`ciaddr`、`giaddr`、`chaddr`，选项 53、55、60、77、93、94、97、175。报文 > 1500 字节或解析失败直接丢弃并计数。
- 判定调用 `xpxe_domain::decision::answer`（Step 3），任何 `Ignore` 都写观测（MAC、架构、user-class、原因），供 Web “发现未登记的 MAC”。
- 限速：每 MAC 每分钟最多 20 个应答，全局每秒 200 个报文；超出丢弃并计数。

### 4.2 应答编码

| 请求 | 应答 |
| :--- | :--- |
| 67 端口 DISCOVER，PXE ROM（BIOS/UEFI） | DHCPOFFER：`yiaddr = 0`；`siaddr` = iface 地址；`file = fw/<sha12>/<undionly.kpxe \| ipxe.efi \| snponly.efi>`；Opt53=OFFER、Opt54=iface 地址、Opt60=`PXEClient`、Opt97 回显（请求携带时）、Opt43 = `{6: 0x08}`（`PXE_DISCOVERY_CONTROL`：直接使用 boot file，不做服务器发现）；按 `DhcpRules.mirror_bootfile_options` 同时写 Opt66/67 |
| 4011 端口 REQUEST（iPXE 收到 Proxy OFFER 后必发；EDK2 `PxeBc` 在 OFFER 已含 Opt43/`siaddr`/`file` 时不发） | DHCPACK，内容与该客户端类型的 OFFER 相同，单播回源地址 |
| 67 端口 DISCOVER，Opt60 = `HTTPClient…`（UEFI HTTP Boot） | DHCPOFFER：Opt60=`HTTPClient`，`file = <XPXE_BOOT_BASE_URL>/fw/x64/ipxe.efi`（URL 含端口），无 Opt43 |
| 67 端口 DISCOVER，Opt77 = `iPXE` 或带 Opt175 | DHCPOFFER：Opt60=`PXEClient`，`siaddr` = iface 地址（iPXE 只在 `siaddr` 非零且有引导文件名时接受 ProxyDHCP 应答），`file = <XPXE_BOOT_BASE_URL>/p/<boot_token>/boot.ipxe`，无 Opt43 |
| 其他（REQUEST/INFORM/RELEASE、非 PXE、中继） | 不应答 |

目的地址：`ciaddr = 0` 或广播标志置位时发往 `255.255.255.255:68`（经 `IP_PKTINFO` 指定出口网卡），否则单播 `ciaddr:68`。

### 4.3 现网 DHCP 探测

- 启动时、模式切换时、此后每 10 分钟，从 `0.0.0.0:68` 以随机本地管理 MAC 发送带 `PXEClient` 厂商类的 DISCOVER，收集 3 秒内的 OFFER（只 DISCOVER，不 REQUEST，不占用租约）。
- 对每个应答者分类：`address_only`（正常现网 DHCP）、`pxe_capable`（携带 `siaddr`/`file`/Opt66/67/43）、`proxy`（`yiaddr = 0` 且 Opt60 = `PXEClient`）。
- `ProxyDhcp` 模式下发现 `pxe_capable` 或 `proxy` 时进入 `Conflict` 状态，暂停应答，Web 显示冲突的服务器地址与选项；管理员确认后可继续（写审计）。
- `ExternalDhcp` 模式下检查现网 DHCP 的 `siaddr` 是否为本机、`file` 是否为预期文件名，Web 显示“已正确指向 / 缺少 next-server / 文件名不符”。

## 5. 只读 TFTP

自行实现，不采用 `async-tftp`：后者面向 async-std/smol 运行时且以文件系统为中心，本系统只需从内存提供少量固件文件，自写协议状态机约 600 行，且能完整受门禁与单元测试约束。

| 项 | 规则 |
| :--- | :--- |
| 协议 | RFC 1350 RRQ（`octet`；`netascii` 拒绝）、RFC 2347 选项协商、RFC 2348 `blksize`（8–1468，按网卡 MTU 计算上限）、RFC 2349 `tsize`/`timeout`、RFC 7440 `windowsize`（1–16） |
| 文件 | 只从内存表查找：`fw/<sha12>/<名称>`（ProxyDHCP 下发的路径）与别名 `undionly.kpxe`、`ipxe.efi`、`snponly.efi`（指向当前激活版本，供现网 DHCP 使用）。查表即可，不存在路径穿越 |
| 写请求 | WRQ 一律返回错误码 2（Access violation） |
| 传输 | 每个传输一个临时 UDP socket（绑定 iface 地址），超时重传最多 5 次；块号 65535 后回绕为 0 |
| 并发 | 全局 64 个、每个源 IP 4 个 |
| 观测 | 每次传输开始/完成/失败（文件、客户端 IP、字节数、耗时）上报 |

## 6. 与控制面的通道

```text
xpxe-bootnet                                  xpxe-control（内部 API，Bearer 令牌）
  GET /internal/v1/bootnet/directives?rev=N ─▶ 有变化立即返回，否则 25 s 后返回 304
  ◀─ { rev, mode, iface, rules, boot_base_url, directives: [{ mac, session, boot_token, firmware: {undionly, efi, snponly}, expires_at }] }
  GET /internal/v1/bootnet/firmware/{sha256} ─▶ 固件字节，bootnet 校验 SHA-256 后放入内存缓存
  POST /internal/v1/bootnet/observations     ─▶ 每 2 s 或满 200 条批量上报
  POST /internal/v1/bootnet/heartbeat        ─▶ 每 10 s，带版本、模式、socket 状态、计数器、探测结论
```

- 指令表以 `tokio::sync::watch` 整体替换，判定始终读到一致快照。
- 连续 30 s 未成功拉取指令：清空指令表（不应答任何人），健康状态降为 `degraded`；恢复后重新拉取。
- 指令里的 `expires_at` 在 bootnet 侧再次检查，控制面推送延迟也不会延长有效期。

## 7. 权限与自检

- 命令行：`xpxe-bootnet run`（常驻进程）；`xpxe-bootnet healthcheck`（容器健康检查，读取 `GET /internal/v1/bootnet/heartbeat`，最近一次心跳在 30 s 内且自报状态为 `ok` 时返回 0）。
- 容器以 UID 10001 运行，`cap_add: [NET_BIND_SERVICE]`。启动时检查内核版本：< 5.7 且未授予 `NET_RAW` 时 `SO_BINDTODEVICE` 会失败，进程以明确错误退出（提示在 compose 中追加 `NET_RAW`）。
- 绑定 67 失败（宿主机已有 DHCP 服务）时报告占用端口的错误并保持 `degraded`，不重试抢占。

## 8. 测试

| 测试 | 内容 |
| :--- | :--- |
| 夹具回放 | Step 2 抓到的每个真实请求报文 → 期望应答（`tests/replay/` 下的同名 `.expect.json`，逐字段比较 `yiaddr`、`siaddr`、`file`、Opt43/54/60/97） |
| 编解码往返 | 自造报文：缺 Opt93、Opt93 多值、Opt77 大小写、超长 Opt60、截断选项 |
| 判定集成 | 内存 `DhcpTransport`：未 arm 不应答；arm 后应答；指令过期后不应答；失联 30 s 后不应答 |
| TFTP 状态机 | 选项协商矩阵、OACK 丢失重传、ACK 重复、窗口、块号回绕（构造 > 32 MiB 内存文件）、WRQ 拒绝、未知文件错误码 1 |
| TFTP 传输 | 回环地址上与测试内客户端完成真实传输，SHA-256 一致 |
| 探测 | 内存传输模拟三类应答者，分类正确；`ProxyDhcp` 遇冲突暂停应答 |
| 模式切换 | 切到 `RedfishOnly` 后 67/69/4011 全部关闭 |

## 9. 验收

```bash
cargo test -p xpxe-dhcp -p xpxe-tftp -p xpxe-bootnet --all-targets --locked
lab/run.sh -j 2 B1 B2 B3 B4 B5 B6 U1 U2 U3 U4 U5 U6 W1 W2 N1 --bootnet target/debug/xpxe-bootnet   # Step 2 矩阵，由正式 bootnet 应答
lab/dhcp.sh start existing && lab/run.sh X1 X2 --bootnet target/debug/xpxe-bootnet               # 复用现有 DHCP 模式
lab/dhcp.sh start rogue && lab/run.sh rogue-proxy   # br-lab 上出现下发引导参数的 lab-rogue，bootnet 必须进入 Conflict
lab/run.sh redfish-only-ports # 切换模式后 ss -ulnp 中没有 :67 :69 :4011
scripts/gate.sh
```

验收标准：

- 实验室场景全部通过，且应答与夹具回放结果一致。
- `rogue-proxy` 场景 bootnet 暂停应答，心跳中报告冲突服务器地址。
- 以 UID 10001 + 仅 `NET_BIND_SERVICE` 在内核 ≥ 5.7 的宿主机上正常运行。
- `scripts/gate.sh` 全绿。

## 10. 实现偏差（落地记录）

| 计划 | 实现 | 理由 |
| :--- | :--- | :--- |
| `IP_PKTINFO` 取收包网卡与目的地址 | 每个 socket `SO_BINDTODEVICE(iface)`；网卡 IPv4 由“设备绑定 socket `connect(255.255.255.255)` 后读取内核选出的源地址”得到 | 只在一块网卡上服务，绑定后收发都限定在该网卡，不必逐包解析辅助数据；网卡需有 scope global 的 IPv4 |
| 快照字段 `mode` + `iface` | `network`：`{"mode":"proxy_dhcp","iface":"br-lab"}` / `external_dhcp` / `redfish_only` | 无网卡的 `redfish_only` 不需要空字段；`iface` 白名单校验在类型上完成 |
| 固件三元组 | 另加按节点的 `uefi_firmware` 覆盖（U6：特定网卡用 `snponly.efi`） | Step 2 U6 场景需要按 MAC 选择 |
| 心跳“socket 状态” | `listening: [67, 4011, 69]` 端口列表 | 直接对应 `redfish-only-ports` 验收 |
| “携带 `siaddr`/`file`/66/67/43” 即 `pxe_capable` | 只有 `siaddr` 不算 | dnsmasq 等普通 DHCP 也写 `siaddr`，会误报冲突；仍要求 `file`/66/67/43 之一 |
| 任何 `Ignore` 都写观测 | 非 PXE 报文（无 `PXEClient`/`HTTPClient`）只计数不写观测 | 避免网段上所有普通 DHCP 流量淹没观测表；未登记 MAC 的 PXE 请求照常记录 `not_armed` |
| 探测使用随机本地管理 MAC | 固定前缀 `02:78:70:78:65`（“xpxe”）+ 随机末字节 | 便于在抓包与其他 DHCP 服务器日志中识别，bootnet 自己也据此忽略探测报文 |
| TFTP 并发超限 | 返回错误码 0（“server busy”） | RFC 1350 无专门错误码 |
| bootnet 启动状态 | 未联系上控制面前按 `redfish_only` 处理（不监听任何端口） | 失联关闭：没有指令就不打开引导端口 |
| 4011 夹具回放 | Step 2 抓包中没有 4011 REQUEST 夹具（iPXE 在实验室均直接接受 67 端口 OFFER），4011 由单元测试构造报文覆盖 | — |
| 控制面内部 API | Step 8 只实现 bootnet 侧客户端；实验室用 `xpxe-lab control-stub` 充当控制面，正式服务端在 Step 10（`XPXE_INTERNAL_LISTEN`）实现 | 控制面在 Step 10 落地 |
| 固件文件名 `undionly.kpxe` 等 | 应答 `file` 为内容寻址路径 `fw/<摘要前缀>/undionly.kpxe`（TFTP 按该路径提供） | 资产更新后客户端不会拿到旧缓存；实验室 B6 断言同时接受两种形式 |

### 实验室验收记录（2026-11-18 Asia/Shanghai，`--bootnet target/debug/xpxe-bootnet`）

- `lab/dhcp.sh start coexist`：B1–B5、U1–U6、W1、W2、N1 PASS（15:44–15:54）；B6 PASS（17:18，修正断言与夹具属主后复跑）。
- `lab/dhcp.sh start existing`：X1、X2 PASS（17:20，external_dhcp 模式，box 未发出任何 DHCP 应答）。
- `lab/dhcp.sh start rogue`：rogue-proxy PASS（health=conflict，心跳列出 10.77.0.3，冲突期间对已武装 MAC 应答 0 次）。
- redfish-only-ports PASS（切到 redfish_only 后 `ss -ulnp` 无 :67/:69/:4011，上报 mode_changed，切回后端口恢复）。
- bootnet 以 UID 10001 + 仅 `NET_BIND_SERVICE`（setpriv ambient caps）运行；`scripts/gate.sh` 全绿，`cargo tree -d` 为空。
