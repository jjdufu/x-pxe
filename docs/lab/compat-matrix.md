# 引导兼容性矩阵（实验室）

本表记录 Step 2 在隔离实验室（`br-lab` 10.77.0.1/24，无上联、无 NAT）上的结论。Step 8/9/13 在正式 `bootnet` / ISO / 控制面替换探针后继续补充。

## 能力边界

QEMU 自带的网卡 Option ROM（Debian `ipxe-qemu` 包，文件在 `/usr/share/qemu/efi-*.rom`）本身就是 iPXE：在 SeaBIOS 下，网卡 ROM 发出的第一个 DHCP 请求已带 Opt77=`iPXE` 与 Opt175；在 OVMF 下，ROM 只提供 SNP 驱动，PXE 客户端是 EDK2 的 `PxeBcDxe` / `HttpBootDxe`。

- **能真实复现**：EDK2 UEFI PXE（Opt93=7，TFTP `ipxe.efi` / `snponly.efi`）、EDK2 UEFI HTTP Boot（Opt93=16，`HTTPClient`）、iPXE 客户端识别与 HTTP 脚本链路、ProxyDHCP 与现网 DHCP 共存、现网 DHCP 下发引导参数、伪造 DHCP 检测、Redfish 虚拟介质、选盘所需的磁盘布局、离线所需的 APT 缓存入口。
- **不能复现**：厂商 BIOS PXE ROM（Intel Boot Agent、Broadcom MBA、Realtek PXE 等）自身的报文差异与缺陷，以及厂商 UEFI 网卡 UNDI 驱动的差异。BIOS 非 iPXE 客户端的 `undionly.kpxe` 链路用“构造报文 + `ipxe.lkrn` 模拟加载”组合覆盖（场景 B6），协议层由 `crates/xpxe-dhcp/tests/fixtures/` 单元测试覆盖。
- 真实硬件兼容矩阵是 v1 之后的可选验收项，不阻塞 v1 发布。

实验室主机：Debian 13，内核 6.12，`/dev/kvm` 属组 `kvm`，QEMU 10.0.13，OVMF 来自 Debian `ovmf` 包，iPXE 1.21.1（Debian `ipxe` / `ipxe-qemu`），Kea 2.6.3，sushy-tools 2.2.0。执行日期：2026-11-14（Asia/Shanghai）。

## 矩阵

| # | 固件 / 网卡 | 链路 | 结果 | 证据 |
| :--- | :--- | :--- | :--- | :--- |
| B1 | SeaBIOS × e1000 | coexist + ProxyDHCP，首包即 iPXE → HTTP 脚本 → d-i | 通过 | `lab/out/B1/result`；夹具 `seabios-e1000-01-discover-67-ipxe`；early 事件 mac=`52:54:00:77:01:b1` |
| B2 | SeaBIOS × e1000e | 同上 | 通过 | `lab/out/B2/result`；`seabios-e1000e-01-discover-67-ipxe` |
| B3 | SeaBIOS × virtio-net | 同上 | 通过 | `lab/out/B3/result`；`seabios-virtio-01-discover-67-ipxe` |
| B4 | SeaBIOS × rtl8139 | 同上 | 通过 | `lab/out/B4/result`；`seabios-rtl8139-01-discover-67-ipxe` |
| B5 | SeaBIOS × vmxnet3 | 同上 | 通过 | `lab/out/B5/result`；`seabios-vmxnet3-01-discover-67-ipxe` |
| B6 | BIOS 非 iPXE（组合模拟） | ① `pxe-client --legacy` 得到 `file=undionly.kpxe`；② SeaBIOS + `ipxe.lkrn` 经 TFTP 加载后再走 iPXE → HTTP | 通过 | `lab/out/B6/result`；夹具 `constructed-bios-discover-67`、`constructed-bios-offer-1`（`siaddr=10.77.0.1`，`file=undionly.kpxe`） |
| U1 | OVMF PXE × e1000 | EDK2 PxeBc：67 端口 OFFER（含 43=`{6:0x08}` / siaddr / file）被直接采用，TFTP `ipxe.efi` → iPXE → HTTP | 通过 | `lab/out/U1/result`；`ovmf-pxe-e1000-01-discover-67`（Opt93=7）；未见 4011 REQUEST，见 H2 |
| U2 | OVMF PXE × e1000e | 同上 | 通过 | `lab/out/U2/result`；`ovmf-pxe-e1000e-*` |
| U3 | OVMF PXE × virtio-net | 同上 | 通过 | `lab/out/U3/result`；`ovmf-pxe-virtio-*` |
| U4 | OVMF PXE × rtl8139 | 同上 | 通过 | `lab/out/U4/result`；`ovmf-pxe-rtl8139-*` |
| U5 | OVMF PXE × vmxnet3 | 同上 | 通过 | `lab/out/U5/result`；`ovmf-pxe-vmxnet3-*` |
| U6 | OVMF PXE × virtio-net、e1000e，规则选 `snponly.efi` | 与 U1 相同链路，固件为 `snponly.efi` | 通过 | `lab/out/U6/result`：两块网卡都进入 d-i，`snponly.efi` 完整传输 2 次。与 `ipxe.efi` 相比，后续同样进入 iPXE HTTP 脚本，实验室中未观察到功能差异 |
| W1 | `ovmf-http` × virtio-net | EDK2 HttpBoot：Proxy OFFER 回 `HTTPClient` + 含端口 URL → HTTP 取 `ipxe.efi` → 脚本 | 通过 | `lab/out/W1/result`；`ovmf-http-virtio-01-discover-67`（Opt93=16） |
| W2 | `ovmf-http` × e1000e | 同上 | 通过 | `lab/out/W2/result`；`ovmf-http-e1000e-01-discover-67` |
| X1 | `existing` × SeaBIOS virtio-net | 现网 DHCP 下发引导参数；探针不发 DHCP | 通过 | `lab/out/X1/result`：进入 d-i；抓包 20 个 DHCP 报文中 `10.77.0.1` 发出 0 个 |
| X2 | `existing` × OVMF e1000e | 同上 | 通过 | `lab/out/X2/result`：进入 d-i；抓包 22 个 DHCP 报文中 box 发出 0 个 |
| N1 | OVMF virtio-net，MAC 不在允许列表 | ProxyDHCP 不应答 | 通过 | `lab/out/N1/result`：应答 0 次，日志 `mac-not-allowed`；`PXE-E16` 后固件转入 IPv6 PXE |
| N2 | OVMF virtio-net 双网卡，从第二块引导 | `BOOTIF` 不足以让 d-i 选中引导网卡；加上 `netcfg/choose_interface=<MAC>` 后通过 | 通过（取舍见 H7） | `lab/out/N2-bootif-only/result`（仅 BOOTIF，d-i 停在 Primary network interface）；`lab/out/N2/result`（附加 choose_interface，early 来自 `52:54:00:77:01:f3`） |
| R1 | Redfish：sushy-tools + libvirt OVMF 域 | InsertMedia → Once/Cd/UEFI → Reset → ISO 内 `autoexec.ipxe` → HTTP 脚本 | 通过 | `lab/out/R1/result`；VirtualMedia 在 Systems 下；early mac=`52:54:00:77:01:a1` |
| R2 | 同 R1，停用 `lab-dhcp`，内核参数静态地址 | iPXE 与 d-i 都不发 DHCP | 通过 | `lab/out/R2/result`：进入 d-i；抓包中该 MAC 的 DHCP 报文 0 个 |
| G1 | `rogue` 与 `coexist` 同时运行 | `xpxe-lab dhcp-scan` | 通过 | `lab/out/G1/result`、`scan.log`：列出 `10.77.0.2`（无引导参数）与 `10.77.0.3`（`file=rogue.pxe`，被标记） |

辅助验证（写入结论表，不单独占矩阵行）：H9a 正确 `preseed/url/checksum` 进入 d-i；H9b 错误 checksum 时 d-i 取到文件但拒绝执行（`lab/out/H9a/result`、`lab/out/H9b/result`）。

## Step 14：手动 ISO 网启（`lab/run.sh manual-sanboot manual-pve`）

测试机：Debian 13 虚拟机（PVE 上开启嵌套 KVM），QEMU 10.0，OVMF 与 SeaBIOS 均来自 Debian 包。引导链路按计划 3.2 节：bootnet 为 `external_dhcp`，现网 DHCP（`lab-dhcp` + 产品生成的 Kea 片段）把 iPXE 指向 `/boot.ipxe`；节点不在 Armed 快照中，走手动菜单。

| # | 固件 / 网卡 | ISO 与方式 | 结果 | 证据 |
| :--- | :--- | :--- | :--- | :--- |
| M1 | OVMF × virtio-net | Debian `ipxe.iso`（识别为 unknown），`sanboot` HTTP 块设备 | 通过 | `lab/out/accept/manual-sanboot/`：串口 `Booting from SAN device 0x80`，随后进入 ISO 内 iPXE；审计 `select` 后再次 `menu_hit` |
| M2 | SeaBIOS × virtio-net | 同 M1 | 通过（证据为审计） | SeaBIOS 下 iPXE 不输出串口；审计 `select` 后约 7 秒再次 `menu_hit`（ISO 内 iPXE 重新请求菜单） |
| M3 | OVMF × virtio-net，8 GiB 内存 | Proxmox VE 9.2 ISO（识别为 PVE，预填 `/boot/linux26` + `/boot/initrd.img`），`kernel_initrd` + `embed_iso` | 通过 | `lab/out/accept/manual-pve/`：内核 `Run /init` 之后串口出现 `Starting the installer GUI`，无 `no cdrom found` |
| M4 | SeaBIOS × virtio-net，8 GiB 内存 | 同 M3 | 通过 | 串口 `Welcome to the Proxmox VE 9.2 (ISO 1) installer!`，无 `no cdrom found` |

说明：`embed_iso` 把整张 ISO（约 1.6 GiB）拼进 initrd，客户机内存需不少于 8 GiB，内核参数带 `ramdisk_size=16777216`。厂商固件对 HTTP `sanboot` 的支持差异不在实验室覆盖范围内，遇到不支持的固件时把条目切换为 `kernel_initrd`。
