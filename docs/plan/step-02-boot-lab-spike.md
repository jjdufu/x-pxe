# Step 02：引导链路技术验证（本机 QEMU/KVM 实验室）

## 1. 目标

在写领域代码和 WebUI 之前，在开发机（下称“box”）上搭一套完全隔离的虚拟实验室，把风险最高的引导假设一次验证完，并把真实报文沉淀成后续单元测试的夹具。本步的产出是“结论表 + 夹具 + 可复跑的实验室”，而不是产品功能。同一套实验室在 Step 8/9 用于集成验证，在 Step 13 承载全部验收场景。

## 2. 实验室主机与边界

| 项 | box 现状 | 实验室用法 |
| :--- | :--- | :--- |
| 系统 | Debian 13（trixie），内核 6.12，PID 1 为 `tini`（无 systemd） | 守护进程（libvirtd、virtlogd）由 `lab/setup.sh` 手动拉起 |
| 虚拟化 | `/dev/kvm` 存在，属组 `kvm`（脚本用 `stat -c %G /dev/kvm` 取实际属组，不写死）；当前用户 `box` 有免密 sudo | 用户 `box` 与 `libvirt-qemu` 加入该属组，QEMU 以非 root 运行并启用 KVM |
| 资源 | 8 核、15 GB 内存、约 58 GB 可用磁盘 | 每台虚拟机 2 vCPU、2 GiB 内存；完整装机场景最多并行 2 台（`lab/run.sh -j 2`），只做引导阶段验证的场景最多并行 2 台（box 同时运行其他容器栈，可用内存约 8 GB）；磁盘为稀疏 qcow2，场景通过后删除（`--keep` 保留），实验室总占用控制在 25 GB 内 |
| 容器 | Docker 29.9.3 | “现网 DHCP”“伪造 DHCP”、APT 缓存、Step 13 的被测产品都以容器运行 |
| 网络 | 实验网桥 `br-lab` 不连接任何物理网卡、不做 NAT | 实验室发出的 DHCP 报文到不了任何真实网络，伪造 DHCP 测试没有外溢风险 |

实验室工具（QEMU、OVMF、libvirt、sushy-tools、Kea、tcpdump）只存在于 box，不进入产品镜像。

### 2.1 能力边界（必须写入 `docs/lab/compat-matrix.md` 首段）

QEMU 自带的网卡 Option ROM（Debian `ipxe-qemu` 包中的 `efi-*.rom`）本身就是 iPXE：在 SeaBIOS 下，网卡 ROM 发出的第一个 DHCP 请求已带 Opt77=`iPXE`/Opt175；在 OVMF 下，ROM 只提供 SNP 驱动，PXE 客户端是 EDK2 的 `PxeBcDxe`/`HttpBootDxe`。因此：

- **能真实复现**：EDK2 UEFI PXE（Opt93=7，67 + 4011 端口，TFTP `ipxe.efi`/`snponly.efi`）、EDK2 UEFI HTTP Boot（Opt93=16，`HTTPClient`）、iPXE 客户端识别与 HTTP 脚本链路、ProxyDHCP 与现网 DHCP 共存、现网 DHCP 下发引导参数、伪造 DHCP 检测、Redfish 虚拟介质、选盘、防重装、离线模式。
- **不能复现**：厂商 BIOS PXE ROM（Intel Boot Agent、Broadcom MBA、Realtek PXE 等）自身的报文差异与缺陷，以及厂商 UEFI 网卡 UNDI 驱动的差异。BIOS 非 iPXE 客户端的 `undionly.kpxe` 链路在实验室里用“构造报文 + iPXE 模拟加载”组合覆盖（场景 B6），协议层另由报文夹具单元测试覆盖。
- 真实硬件兼容矩阵是 v1 之后的可选验收项（Step 13 第 6.2 节），不阻塞 v1 发布。

## 3. 交付物

| 路径 | 内容 |
| :--- | :--- |
| `lab/setup.sh` | 幂等：安装软件包、加组、生成 sushy 证书与口令、启动 libvirtd/virtlogd、构建实验室镜像 |
| `lab/net.sh up\|down\|selftest` | 建/拆 `br-lab`、tap 设备与隔离规则；`selftest` 建临时命名空间 `ns-probe` 验证隔离 |
| `lab/dhcp.sh start <existing\|coexist\|rogue>\|stop` | 以容器运行 Kea，并把 veth 接入 `br-lab` |
| `lab/vm.sh` | QEMU 启动封装：固件（SeaBIOS/OVMF/OVMF-HTTP）、网卡型号、磁盘布局、引导顺序 |
| `lab/run.sh` | 场景编排：`lab/run.sh [-j N] <场景…>\|all`，每个场景输出 PASS/FAIL、耗时与日志目录 |
| `lab/images/kea/Dockerfile`、`lab/kea/*.json` | 现网 DHCP 与伪造 DHCP（`debian:trixie-slim` + `kea-dhcp4-server`） |
| `lab/images/acng/Dockerfile` | APT 缓存（`apt-cacher-ng`），供虚拟机装机与离线场景使用 |
| `lab/sushy/` | sushy-tools 配置、证书、口令文件；`lab/libvirt/*.xml` 为 Redfish 场景的 libvirt 域 |
| `crates/xpxe-lab` | 实验室探针（第 7 节） |
| `crates/xpxe-dhcp/tests/fixtures/` | 抓到的 DHCP 报文（二进制 + 同名 `.json` 描述：来源固件、网卡、阶段） |
| `docs/lab/spike-report.md` | 结论表（第 6 节每一项：通过/不通过、证据、后续步骤的设计取舍） |
| `docs/lab/compat-matrix.md` | 兼容性矩阵（第 5 节），Step 8/9/13 持续补充 |

## 4. 搭建

### 4.1 软件包、KVM 权限与守护进程（`lab/setup.sh` 的内容）

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  qemu-system-x86 qemu-utils ovmf ipxe-qemu ipxe seabios \
  libvirt-daemon-system libvirt-clients python3-libvirt python3-venv \
  iproute2 iptables tcpdump socat openssl apache2-utils

# KVM：属组取 /dev/kvm 的实际属组（box 上为 kvm）
KVM_GROUP="$(stat -c %G /dev/kvm)"
sudo usermod -aG "$KVM_GROUP" box            # 新登录会话生效
sudo usermod -aG "$KVM_GROUP" libvirt-qemu   # libvirt 拉起的 QEMU
sg "$KVM_GROUP" -c 'test -r /dev/kvm && test -w /dev/kvm' && echo kvm-ok
# 当前 shell 未重新登录时，实验室脚本统一经 `sg "$KVM_GROUP" -c` 运行 QEMU；
# 不使用 `sudo qemu-system-x86_64`，避免虚拟机进程以 root 运行。

# 无 systemd：手动启动 libvirt
sudo virtlogd -d
sudo libvirtd -d
virsh -c qemu:///system version
# 若 libvirtd 报 cgroup 错误：在 /etc/libvirt/qemu.conf 设 cgroup_controllers = [ ] 后重启 libvirtd
# libvirt 自带的 default 网络带 NAT 出口，实验室不使用：
virsh -c qemu:///system net-destroy default 2>/dev/null || true
virsh -c qemu:///system net-autostart default --disable 2>/dev/null || true

# sushy-tools（版本在 lab/sushy/requirements.txt 中固定）
python3 -m venv --system-site-packages /opt/xpxe-lab/sushy
/opt/xpxe-lab/sushy/bin/pip install -r lab/sushy/requirements.txt
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 30 \
  -subj /CN=lab-bmc -keyout lab/sushy/key.pem -out lab/sushy/cert.pem
htpasswd -nbB admin "$(openssl rand -hex 12)" > lab/sushy/htpasswd   # 口令同时写入 lab/sushy/password（0600）

# 实验室镜像
docker build -t xpxe-lab/kea  lab/images/kea
docker build -t xpxe-lab/acng lab/images/acng
```

固件文件位置（Debian 包提供）：SeaBIOS 为 QEMU 默认 BIOS；`/usr/share/OVMF/OVMF_CODE_4M.fd` + `OVMF_VARS_4M.fd`（每台虚拟机复制一份 VARS）；网卡 ROM 为 `/usr/share/qemu/efi-{e1000,e1000e,virtio,rtl8139,vmxnet3}.rom`（`ipxe-qemu` 包，QEMU 按网卡型号自动加载）；`ipxe` 包提供 `/usr/lib/ipxe/{ipxe.efi,snponly.efi,undionly.kpxe,ipxe.lkrn}`，实验室的 TFTP/HTTP 根目录从这里复制（不入库），`ipxe.lkrn` 只用于场景 B6。

### 4.2 隔离网络（`lab/net.sh up`）

```bash
sudo ip link add br-lab type bridge stp_state 0 forward_delay 0
sudo ip addr add 10.77.0.1/24 dev br-lab
sudo ip link set br-lab up
for i in 0 1 2 3; do
  sudo ip tuntap add tap$i mode tap user box
  sudo ip link set tap$i master br-lab up
done
# 另建一个不接入网桥的孤立 tap（tap-iso），作为 N2 双网卡场景的第一块网卡：有链路、无 DHCP、无出口
sudo ip tuntap add tap-iso mode tap user box && sudo ip link set tap-iso up
# 不加入任何物理网卡；box 上已有的 FORWARD 规则会放行 br-+，因此在最前面插入两条隔离规则：
sudo iptables -I FORWARD 1 -i br-lab ! -o br-lab -m conntrack ! --ctstate DNAT -j DROP
sudo iptables -I FORWARD 1 -o br-lab ! -i br-lab -m conntrack ! --ctstate RELATED,ESTABLISHED -j DROP
# 不修改全局 sysctl bridge-nf-call-iptables：box 上还运行着其他 Docker 栈，
# 即使加载了 br_netfilter，桥内二层流量也只匹配 -i br-lab -o br-lab，不会被上面两条规则丢弃。
```

- 虚拟机只能访问 `10.77.0.1` 上发布的端口（被测控制面 8080、APT 缓存 3142、sushy 8000），这些流量经 Docker DNAT 进入容器，被第一条规则的 `DNAT` 例外放行；虚拟机无法访问外网。
- 自检：`ip link show master br-lab` 只列出 tap 与实验室 veth；`lab/net.sh selftest` 在临时网络命名空间 `ns-probe`（veth 接入 `br-lab`，地址 10.77.0.9）中 `ping -c1 -W1 1.1.1.1` 必须失败、`curl http://10.77.0.1:3142` 必须成功。
- `lab/net.sh down` 删除 tap、veth、网桥与两条规则，恢复 box 原状。

### 4.3 “现网 DHCP”与伪造 DHCP（`lab/dhcp.sh`）

DHCP 服务器以容器运行，网络用 `--network none`，再把 veth 的一端移入容器网络命名空间：

```bash
docker run -d --name lab-dhcp --network none \
  -v "$PWD/lab/kea/$PROFILE.json:/etc/kea/kea-dhcp4.conf:ro" xpxe-lab/kea
pid=$(docker inspect -f '{{.State.Pid}}' lab-dhcp)
sudo ip link add vh-dhcp type veth peer name vc-dhcp
sudo ip link set vh-dhcp master br-lab up
sudo ip link set vc-dhcp netns "$pid"
sudo nsenter -t "$pid" -n sh -c 'ip link set vc-dhcp name eth0; ip addr add 10.77.0.2/24 dev eth0; ip link set eth0 up'
```

| 配置 | 地址 | 行为 | 用途 |
| :--- | :--- | :--- | :--- |
| `coexist.json` | 10.77.0.2，池 .100–.149 | 只分配地址、网关、DNS，不带任何引导选项 | ProxyDHCP 与现网 DHCP 共存（默认） |
| `existing.json` | 10.77.0.2，池 .100–.149 | 按客户端类别下发引导参数：Opt77=`iPXE` → `http://10.77.0.1:8080/boot.ipxe`；Opt93=7 → `next-server 10.77.0.1` + `ipxe.efi`；Opt93=0 → `undionly.kpxe`；Opt93=16 → `HTTPClient` + `http://10.77.0.1:8080/fw/x64/ipxe.efi`。内容与产品“复用现有 DHCP”页面给出的 Kea 片段一致（Step 8 起由 `lab/run.sh` 从 API 取片段生成） | 复用现有 DHCP 模式 |
| `rogue.json` | 10.77.0.3（容器 `lab-rogue`），池 .150–.199 | 分配地址并下发 `next-server`/`filename` | 伪造 DHCP 与冲突检测 |

Kea 是实验室中“现网 DHCP”的替身，不属于产品；产品内只有 Rust 实现的 ProxyDHCP。

### 4.4 APT 缓存

```bash
docker run -d --name lab-acng -p 10.77.0.1:3142:3142 -v xpxe-lab-acng:/var/cache/apt-cacher-ng xpxe-lab/acng
```

实验室 Profile 的镜像源为 `http://10.77.0.1:3142/deb.debian.org/debian`。首次装机时缓存从公网回源；离线场景（O1）先预热，再把 `apt-cacher-ng` 切到 `Offlinemode: 1` 只用缓存。

### 4.5 虚拟机（`lab/vm.sh`）

```bash
# 例：OVMF + e1000e，网卡优先引导，标准磁盘布局
sg pulse-access -c "qemu-system-x86_64 -accel kvm -machine q35 -cpu host -smp 2 -m 2048 \
  -drive if=pflash,format=raw,unit=0,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,unit=1,file=$VM/vars.fd \
  -netdev tap,id=n0,ifname=tap1,script=no,downscript=no \
  -device e1000e,netdev=n0,mac=52:54:00:77:01:05,bootindex=1 \
  -drive if=none,id=sys,file=$VM/sys.qcow2 -device virtio-blk-pci,drive=sys,serial=SYS0001,bootindex=2 \
  -drive if=none,id=data,file=$VM/data.qcow2 -device virtio-blk-pci,drive=data,serial=DATA0001 \
  -device qemu-xhci -drive if=none,id=vusb,format=raw,file=$VM/vusb.raw \
  -device usb-storage,drive=vusb,removable=on,serial=VUSB0001 \
  -serial file:$VM/serial.log -display none -monitor unix:$VM/mon.sock,server,nowait"
```

| 参数 | 取值 |
| :--- | :--- |
| `--fw` | `seabios`（QEMU 默认）、`ovmf`、`ovmf-http`（追加 `-fw_cfg name=opt/org.tianocore/IPv4PXESupport,string=n` 与 `IPv6PXESupport,string=n`，只剩 HTTP Boot 启动项） |
| `--nic` | `e1000`、`e1000e`、`virtio-net-pci`、`rtl8139`、`vmxnet3`；`--nic2` 追加第二块网卡 |
| 磁盘布局 | `sys` 20 GiB virtio（系统盘）、`data` 60 GiB virtio（数据盘）、`vusb` 1 GiB USB 可移动盘（模拟 BMC 虚拟 U 盘），全部稀疏；`--disks` 可改容量用于选盘场景 |
| 引导顺序 | `--boot net-first`（网卡 `bootindex=1`）或 `disk-first` |
| 观测 | 串口日志（实验室 Profile 内核参数含 `console=ttyS0,115200`）、QEMU monitor、`tcpdump -i br-lab -w` 抓包 |

### 4.6 Redfish 模拟

Redfish 场景的虚拟机由 libvirt 定义（`lab/libvirt/vm-redfish.xml`：`<os firmware='efi'>`、网卡 `type='bridge'` 接 `br-lab`、同 4.5 的磁盘布局），sushy-tools 以 libvirt 驱动管理它：

```bash
/opt/xpxe-lab/sushy/bin/sushy-emulator --config lab/sushy/emulator.conf
# emulator.conf：SUSHY_EMULATOR_LISTEN_IP='10.77.0.1'、PORT=8000、LIBVIRT_URI='qemu:///system'、
#                SSL_CERT/SSL_KEY=lab/sushy/{cert,key}.pem、AUTH_FILE=lab/sushy/htpasswd、
#                ALLOWED_INSTANCES=[vm-redfish 的 UUID]
```

sushy-tools 的 VirtualMedia 会从给定 URL 下载 ISO 并以光驱挂到域上，`BootSourceOverrideTarget=Cd` 改写域的引导顺序。Redfish 只支持 UEFI，实验室中没有 SeaBIOS 的 Redfish 域。

## 5. 兼容性矩阵（全部在 box 上执行，必须全部有结论）

| # | 固件 / 网卡 | 链路 | 期望 |
| :--- | :--- | :--- | :--- |
| B1–B5 | SeaBIOS × e1000 / e1000e / virtio-net / rtl8139 / vmxnet3（QEMU iPXE ROM） | `coexist` 分配地址；首个 DHCP 即 iPXE → ProxyDHCP 直接给 HTTP 脚本 URL → d-i | 进入 d-i，preseed 被取走 |
| B6 | BIOS 非 iPXE 客户端（组合模拟） | ① `xpxe-lab pxe-client --legacy` 从 `ns-probe` 发 Opt93=0、无 Opt77/175 的 DISCOVER，应答 `file=undionly.kpxe`；② SeaBIOS 虚拟机以 `-kernel /usr/lib/ipxe/ipxe.lkrn -initrd lab/ipxe/legacy-sim.ipxe` 启动，脚本 `dhcp && chain tftp://10.77.0.1/<①中的 file>`，经 TFTP 加载 `undionly.kpxe`，后者再走 iPXE → HTTP 脚本 | ① 报文断言通过；② 进入 d-i |
| U1–U5 | OVMF PXE × e1000 / e1000e / virtio-net / rtl8139 / vmxnet3 | EDK2 PxeBc：67 端口 OFFER → 4011 REQUEST/ACK → TFTP `ipxe.efi` → iPXE → HTTP 脚本 | 进入 d-i |
| U6 | OVMF PXE × virtio-net、e1000e，规则选 `snponly.efi` | 同上，固件为 `snponly.efi` | 进入 d-i；记录与 `ipxe.efi` 的差异 |
| W1–W2 | `ovmf-http` × virtio-net / e1000e | EDK2 HttpBoot：ProxyDHCP 回 `HTTPClient` + 含端口 URL → `ipxe.efi` → HTTP 脚本 | 进入 d-i |
| X1–X2 | `existing` × SeaBIOS virtio-net / OVMF e1000e | 现网 DHCP 下发引导参数，探针只跑 TFTP + HTTP，不发任何 DHCP | 进入 d-i；抓包中 box 不发 DHCP 报文 |
| N1 | 任一固件，MAC 不在允许列表 | 不应答 | 固件超时后回本地盘或下一启动项 |
| N2 | OVMF virtio-net 双网卡，从第二块网卡引导 | `BOOTIF` 选中引导网卡 | d-i 在第二块网卡上配置网络 |
| R1 | Redfish：sushy-tools + libvirt OVMF 域 | InsertMedia → Once/Cd/UEFI → Reset → ISO 内 `autoexec.ipxe` → HTTP 脚本 | 进入 d-i |
| R2 | 同 R1，停用 `lab-dhcp`，内核参数静态地址 | iPXE 与 d-i 都不发 DHCP | 进入 d-i 并取到 preseed |
| G1 | `rogue` 与 `coexist` 同时运行 | `xpxe-lab dhcp-scan` | 列出两台服务器，`lab-rogue` 被标记为下发引导参数 |

B1–B6、U1–U6、W1–W2、X1–X2 的每个首包与关键包都保存为夹具。

## 6. 必须验证的假设

| # | 假设 | 验证方法 | 不成立时的设计取舍 |
| :--- | :--- | :--- | :--- |
| H1 | ProxyDHCP OFFER 只需 Opt60=`PXEClient`、Opt54、Opt97（回显 GUID）、Opt43（子选项 6 = 0x08，跳过发现）+ `siaddr`/`file`，EDK2 PXE 接受 | U1–U5 抓包对比 | 补发 Opt66/67，或只在 4011 的 ACK 里给 `file` |
| H2 | EDK2 PXE 客户端在 4011 端口发 REQUEST，必须在 4011 应答 | U1–U5 | - |
| H3 | EDK2 HTTP Boot 接受 Proxy 形式的 OFFER（不含地址），URL 带端口有效 | W1–W2 | 不接受时 HTTP Boot 只在“复用现有 DHCP”模式下支持，文档写明 |
| H4 | iPXE 的 DHCP 同时带 Opt77=`iPXE` 与 Opt175；只认其一会漏判 | B1–B5、U1 二次 DHCP 抓包 | 判定条件取“二者任一” |
| H5 | 官方 `ipxe.efi` 从 ISO 内 ESP（FAT）启动时读取同目录 `autoexec.ipxe` | R1 | Redfish 通道降级为“一次性网络引导”：`BootSourceOverrideTarget=Pxe` + LAN PXE，ISO 方案暂停并标记风险 |
| H6 | d-i 在拉取 preseed 之前，按内核参数 `netcfg/disable_autoconfig=true netcfg/get_ipaddress=… netcfg/get_netmask=… netcfg/get_gateway=… netcfg/get_nameservers=… netcfg/confirm_static=true` 完成静态配置 | R2 | - |
| H7 | iPXE 传 `BOOTIF=01-${net0/mac:hexhyp}` 时，多网卡机器上 netcfg 选中引导网卡 | N2 | 采用 `netcfg/choose_interface=<网卡名>`，节点规格增加网卡名字段 |
| H8 | d-i 的 busybox `wget` 支持 `--post-data` 与 `--header`，能在 `early_command`/`late_command` 中 POST 事件 | U1 中 d-i 的 shell（串口） | 采用 `nc` 发送原始 HTTP 请求的模板 |
| H9 | `preseed/url/checksum=<md5>` 生效，内容被篡改时 d-i 拒绝 | 改一个字节 | - |
| H10 | `dists/trixie/InRelease` 的 `SHA256` 段列出 `main/installer-amd64/current/images/SHA256SUMS`，后者覆盖 `netboot/debian-installer/amd64/linux` 与 `initrd.gz` | 下载并离线校验 | 校验 `netboot.tar.gz` 后再解包 |
| H11 | 从 bzImage 头（`kernel_version` 偏移 0x20E）读出的版本字符串可与镜像源 `kernel-image-<abi>-di` udeb 对齐 | 读取 trixie netboot `linux` | 解析 initrd 中 `lib/modules/<abi>` 目录名 |
| H12 | sushy-tools 的 VirtualMedia 行为（Systems 下 vs Managers 下、`TransferProtocolType`）与 iDRAC/iLO 的差异可被一个“厂商画像”结构覆盖 | 对照 DMTF 规范与厂商文档建表 | - |
| H13 | `xpxe-image` 自写的 ISO9660 + El Torito 写出器生成的 ISO（单个 UEFI 0xEF no-emulation 项指向 FAT 镜像）能在 OVMF 启动 | R1 | 调整 ISO 布局（ESP 对齐、引导目录位置）直到 OVMF 接受，结论写入结论表 |
| H14 | 内核 ≥ 5.7 时，`SO_BINDTODEVICE` 不需要 `NET_RAW`（box 内核 6.12） | 在 UID 10001 + 仅 `NET_BIND_SERVICE` 下运行 `xpxe-lab proxydhcp`（`sudo setpriv --reuid 10001 --inh-caps +net_bind_service --ambient-caps +net_bind_service`） | compose 为 bootnet 追加 `NET_RAW` 并在文档写明 |
| H15 | `ipxe.lkrn` 提供的 PXE API 足以运行 `undionly.kpxe`（B6 第②步） | B6 | B6 第②步只断言 TFTP 传输字节一致，BIOS 非 iPXE 链路的端到端验证列入真实硬件可选项 |
| H16 | OVMF 的 `opt/org.tianocore/IPv4PXESupport=n` 关闭 PXE 启动项后只剩 HTTP Boot | W1–W2 启动菜单与抓包 | 用 `virt-fw-vars` 编辑 VARS 中的启动顺序 |
| H17 | 无 systemd 的 box 上手动启动的 libvirtd 能拉起 KVM 域，sushy-tools 能挂载虚拟介质 | R1 | 在 `xpxe-lab` 中增加 `redfish-shim`：最小 Redfish 服务，经 QMP 切换光驱介质、设置一次性引导并复位 QEMU（同样受门禁约束） |

## 7. `xpxe-lab` 探针

探针代码同样受门禁约束，后续步骤的正式实现会替换它的内部，但子命令保留用于现场排障：

| 子命令 | 行为 |
| :--- | :--- |
| `capture --pcap <file> --out <dir> --label <前缀> [--mac <mac>…] [--first-per-phase]` | 解析 `tcpdump -i br-lab -w` 的经典 pcap，提取 67/68/4011 的 DHCP 报文原样保存并生成描述 JSON（门禁禁止 `unsafe`，探针不自开原始套接字） |
| `proxydhcp --iface br-lab --allow-mac <mac> --boot-base http://10.77.0.1:8080` | 最小 ProxyDHCP，只应答允许的 MAC |
| `tftp --root-file undionly.kpxe=<path> --root-file ipxe.efi=<path>` | 最小只读 TFTP，只服务列出的文件 |
| `iso --efi <ipxe.efi> --script <autoexec.ipxe> --out boot.iso` | UEFI 引导 ISO 试制 |
| `dhcp-scan --iface br-lab` | 从 68 端口发 DISCOVER，列出所有应答的 DHCP 服务器及其引导相关选项 |
| `pxe-client --iface <if> --legacy\|--uefi\|--http` | 发出指定类型的 PXE DISCOVER（及 4011 REQUEST），打印应答 |

最小 HTTP 端（脚本、内核、initrd、preseed）在本步用 `xpxe-lab serve --dir lab/www --listen 10.77.0.1:8080` 提供；`lab/www/preseed.cfg` 手写一份最小可装机 preseed（镜像源指向 4.4 的 APT 缓存），作为 Step 4 渲染结果的对照基准。

## 8. 验收

```bash
lab/setup.sh && lab/net.sh up && lab/net.sh selftest   # 隔离自检
lab/dhcp.sh start coexist
lab/run.sh -j 2 B1 B2 B3 B4 B5 B6 U1 U2 U3 U4 U5 U6 W1 W2 N1 N2 H9a H9b
lab/dhcp.sh start existing && lab/run.sh X1 X2
lab/dhcp.sh stop && lab/run.sh R2 && lab/dhcp.sh start coexist && lab/run.sh R1
lab/dhcp.sh start rogue && lab/run.sh G1
lab/run.sh capture                                   # 生成夹具
ls crates/xpxe-dhcp/tests/fixtures/*.bin | wc -l     # 至少覆盖：SeaBIOS iPXE（5 种网卡）、OVMF PXE（5 种网卡）、OVMF HTTP、4011 REQUEST、构造的 BIOS 非 iPXE DISCOVER
scripts/gate.sh
lab/net.sh down
```

验收标准：

- 第 5 节矩阵每一项都有结论；B1–B5、U1–U5、W1–W2、X1–X2、N1、N2、R1、R2、G1 通过；B6、U6 通过或按第 6 节取舍记录。
- H1–H17 在 `spike-report.md` 中逐条给出结论与证据（抓包文件名、串口日志行、截图），每条“不成立”都写明后续步骤按哪个取舍执行。
- `lab/net.sh selftest` 证明 `br-lab` 无外网出口；`lab/net.sh down` 后 `ip link`、`iptables -S FORWARD` 与搭建前一致。
- `scripts/gate.sh` 全绿。
