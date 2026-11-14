# Step 2 引导链路假设验证结论

实验室环境、场景定义与取舍原则见 `docs/plan/step-02-boot-lab-spike.md`。兼容性矩阵见 `docs/lab/compat-matrix.md`。抓包夹具在 `crates/xpxe-dhcp/tests/fixtures/`（107 个 `.bin` + 同名 `.json`）。本文件记录 H1–H17 的结论与证据。执行日期：2026-11-14（Asia/Shanghai）。

判定“进入 d-i”的标准：HTTP 服务收到 `preseed.cfg` 的 GET，并且 d-i `early_command` 用 busybox `wget --post-data` 向 `/events/early` 上报了对应 MAC。

## H1–H17

| # | 假设 | 结论 | 证据与取舍 |
| :--- | :--- | :--- | :--- |
| H1 | ProxyDHCP OFFER 只需 Opt60=`PXEClient`、Opt54、Opt97（回显 GUID）、Opt43（子选项 6 = 0x08）+ `siaddr`/`file`，EDK2 PXE 接受 | **成立** | U1–U6：EDK2 首包 `ovmf-pxe-*-01-discover-67`（Opt93=7）之后出现 iPXE 二次 DHCP（`*-05-discover-67-ipxe`），TFTP 传完 `ipxe.efi`/`snponly.efi` 并进入 d-i。不需要补发 Opt66/67。 |
| H2 | EDK2 PXE 客户端在 4011 端口发 REQUEST，必须在 4011 应答 | **不成立（对实验室中的 EDK2 PxeBc）** | U1–U6 与 W 的抓包中均无 4011 REQUEST。EDK2 在 67 端口收到已含 Opt43/`siaddr`/`file` 的 Proxy OFFER 后直接采用，随后 TFTP。iPXE 则仍会向 4011 发 REQUEST（B 组与二次 iPXE 会话）。**后续步骤**：4011 ACK 仍实现并保持，以覆盖 iPXE 以及可能仍走 4011 的真实固件；不得把“必须先有 4011 REQUEST 才给 file”写成 EDK2 的前置条件。 |
| H3 | EDK2 HTTP Boot 接受 Proxy 形式的 OFFER（不含地址），URL 带端口有效 | **成立** | W1/W2：首包 Opt93=16、`HTTPClient:Arch:00016:UNDI:003001`；HTTP 服务以 200 返回 `/fw/x64/ipxe.efi`；随后 iPXE 二次 DHCP 并进入 d-i。关闭 PXE 启动项后无 4011 流程（H16）。 |
| H4 | iPXE 的 DHCP 同时带 Opt77=`iPXE` 与 Opt175；只认其一会漏判 | **成立** | 所有 SeaBIOS 首包与 OVMF 二次 DHCP 的 `*-discover-67-ipxe.json` 均 `user_class=iPXE` 且 `opt175=true`。判定条件保持“二者任一”（仅 Opt175 的单元测试见 `xpxe-lab` 的 `decisions`）。 |
| H5 | 官方 `ipxe.efi` 从 ISO 内 ESP（FAT）启动时读取同目录 `autoexec.ipxe` | **成立（根目录与同目录均可）** | 单独验证：`--script-dir root`、`efi`、`both` 三种 ESP 布局在 OVMF 下均打印 `AUTOEXEC-OK`。R1/R2 使用 `both`，ISO 内脚本被执行并进入 d-i。无需降级为“一次性网络引导”。 |
| H6 | d-i 在拉取 preseed 之前，按内核参数 `netcfg/disable_autoconfig=true` 与 `netcfg/get_*` 完成静态配置 | **成立** | R2：停用 `lab-dhcp`，autoexec 给 iPXE 静态地址 `10.77.0.60`，内核参数给 d-i 同一组静态配置。进入 d-i；抓包中该 MAC 的 DHCP 报文 0 个（`lab/out/R2/result`）。 |
| H7 | iPXE 传 `BOOTIF=01-${net0/mac:hexhyp}` 时，多网卡机器上 netcfg 选中引导网卡 | **不成立** | N2：第一块网卡接孤立 tap（无 DHCP），第二块（`52:54:00:77:01:f3`）优先引导。内核命令行已含正确 `BOOTIF=01-52-54-00-77-01-f3`，但 d-i 仍停在 “Primary network interface”（`enp0s2`/`enp0s3` Unknown interface，见 `lab/out/N2-bootif-only/serial-f3.log`）。**取舍已执行**：节点脚本附加 `netcfg/choose_interface=<引导网卡 MAC>` 后进入 d-i（`lab/out/N2/result`）。后续步骤节点规格增加该字段（或由 boot 脚本按引导 MAC 写入）。 |
| H8 | d-i 的 busybox `wget` 支持 `--post-data` 与 `--header`，能在 `early_command` 中 POST 事件 | **成立** | 全部进入 d-i 的场景都在 `xpxe-lab serve` 日志中留下 `{"event":"post","path":"/events/early","body":"event=early mac=…"}`。无需改用 `nc`。 |
| H9 | `preseed/url/checksum=<md5>` 生效，内容被篡改时 d-i 拒绝 | **成立** | H9a：正确 MD5，进入 d-i（36s）。H9b：故意错误的 MD5，HTTP 取到 `preseed.cfg` 但无 early 事件（184s 超时，`lab/out/H9b/result`）。 |
| H10 | `dists/trixie/InRelease` 的 `SHA256` 段列出 `main/installer-amd64/current/images/SHA256SUMS`，后者覆盖 netboot `linux` 与 `initrd.gz` | **成立** | `lab/setup.sh` 用 `sqv` 校验 InRelease（`debian-archive-keyring`），再核对 SHA256SUMS 的摘要 `31a1a4fc99c5b0a729d261f137cd7e1599bfa6f214f83fe4e596543d8ccf2dec`，以及 `linux` / `initrd.gz` 与 SHA256SUMS 中的哈希一致。 |
| H11 | 从 bzImage 头（`kernel_version` 偏移 0x20E）读出的版本字符串可与镜像源 `kernel-image-<abi>-di` udeb 对齐 | **成立** | netboot `linux` 读出 `6.12.107+deb13-amd64`；`debian-installer` 组件仓库中存在 `kernel-image-6.12.107+deb13-amd64-di`（另有较旧的 `6.12.94+deb13-amd64-di`）。后续校验按此对齐，不必先解析 initrd 目录名。 |
| H12 | sushy-tools 的 VirtualMedia 行为（Systems 下 vs Managers 下、`TransferProtocolType`）与 iDRAC/iLO 的差异可被一个“厂商画像”结构覆盖 | **成立（实验室画像已记下）** | sushy-tools 2.2.0：`VirtualMedia` 集合挂在 `/redfish/v1/Systems/<uuid>/VirtualMedia`（`Cd`）；Managers 资源里的 `VirtualMedia.@odata.id` 也指向 Systems 路径。InsertMedia 接受 `Image` / `Inserted` / `WriteProtected`，成功返回 204，并把 ISO 放到 libvirt `default` 存储池后挂到域的 SATA 光驱。后续“厂商画像”至少区分：介质集合挂在 Systems 还是 Managers、设备名（`Cd` / `VirtualCD` 等）、是否需要 `TransferProtocolType`。 |
| H13 | `xpxe-image` 自写的 ISO9660 + El Torito 写出器生成的 ISO（单个 UEFI 0xEF no-emulation 项指向 FAT 镜像）能在 OVMF 启动 | **成立** | 实验室 `xpxe-lab iso` 生成的 ISO 被 `file` 识别为 `ISO 9660 … (bootable)`；OVMF 从 QEMU DVD-ROM 启动官方 `ipxe.efi` 并执行 `autoexec.ipxe`（H5）。R1/R2 经 Redfish 虚拟介质走同一写出器，同样进入 d-i。 |
| H14 | 内核 ≥ 5.7 时，`SO_BINDTODEVICE` 不需要 `NET_RAW`（box 内核 6.12） | **成立** | `lab/run.sh` 用 `setpriv --reuid 10001 --regid 10001 --clear-groups --inh-caps +net_bind_service --ambient-caps +net_bind_service` 启动 `proxydhcp` 与 `tftp`。`xpxe-lab tftp` 进程 UID=10001，`CapEff` 仅 `net_bind_service`；绑定 `10.77.0.1:69` 并完成 `ipxe.efi` / `snponly.efi` / `undionly.kpxe` 传输。`proxydhcp` 以同样方式绑定 67/4011，B/U/W 矩阵全部通过。产品 compose **不必** 为 bootnet 追加 `NET_RAW`。 |
| H15 | `ipxe.lkrn` 提供的 PXE API 足以运行 `undionly.kpxe`（B6 第②步） | **成立** | B6：`ipxe.lkrn` + `legacy-sim.ipxe` 经 TFTP 完整加载 `undionly.kpxe`，随后进入 d-i（`lab/out/B6/result`）。BIOS 非 iPXE 的端到端验证仍可作为真实硬件可选项补充，但不阻塞实验室结论。 |
| H16 | OVMF 的 `opt/org.tianocore/IPv4PXESupport=n` 关闭 PXE 启动项后只剩 HTTP Boot | **成立** | `lab/vm.sh --fw ovmf-http` 追加 IPv4/IPv6 PXESupport=n。W1/W2 首包即为 Opt93=16 的 HTTPClient DISCOVER，抓包中无 PXE 4011 流程。 |
| H17 | 无 systemd 的 box 上手动启动的 libvirtd 能拉起 KVM 域，sushy-tools 能挂载虚拟介质 | **成立** | `lab/setup.sh` 手动 `virtlogd -d` / `libvirtd -d`（`cgroup_controllers = []`），并定义 dir 类型的 libvirt `default` 存储池（sushy-tools 下载 ISO 需要，与已停用的 default NAT 网络无关）。R1/R2：Redfish 列出 1 个系统，InsertMedia / BootOverride / Reset 成功，虚拟机进入 d-i。不必启用 `redfish-shim`。 |

## 协议层补充结论（影响 Step 8）

- **iPXE 接受 ProxyDHCP 应答的条件**是 `siaddr` 非零且存在引导文件名（或 PXE 启动菜单），即 iPXE 源码中的 `dhcp_has_pxeopts`。脚本 URL 走 HTTP 时也必须填写 `siaddr`（实验室 B3 第一次失败的原因：`IpxeScript` 把 `siaddr` 留空，iPXE 反复向 4011 要 ACK 却仍不采用）。`xpxe-dhcp::proxy::build_reply` 对 `IpxeScript` 写入本机地址。Step 8 的正式 bootnet 必须保持这一行为。
- EDK2 PxeBc 在 67 端口的 Proxy OFFER 已经足够（H1、H2）；HTTP Boot 同理（H3）。

## 实验室实现要点

- DHCP 夹具由 `tcpdump -i br-lab -w` 抓取，再经 `xpxe-lab capture --pcap` 离线提取（门禁禁止 `unsafe`，探针不自开原始套接字）。
- `lab/net.sh selftest`：宿主能访问 1.1.1.1，但 `ns-probe`（10.77.0.9）不能 ping / HTTP 访问 1.1.1.1，能访问 `10.77.0.1` 以及经 DNAT 发布的 APT 缓存 `10.77.0.1:3142`。
- 网卡 ROM 路径为 `/usr/share/qemu/efi-*.rom`；`/dev/kvm` 属组以 `stat -c %G` 为准（本机为 `kvm`）。
- 并行度默认最多 2 台（开发机同时运行其他容器栈）。N2 使用不接入网桥的孤立 tap `tap-iso` 作为第一块网卡。
