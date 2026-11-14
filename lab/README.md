# lab

开发机上的隔离 QEMU/KVM 引导实验室：SeaBIOS/OVMF 虚拟机、隔离网桥 `br-lab`（无上联、无 NAT）、Kea 现网 DHCP 替身、sushy-tools Redfish 模拟器、抓包夹具与兼容性记录。

## 入口

```bash
lab/setup.sh                 # 幂等安装依赖、权限、libvirtd、sushy、d-i 与 iPXE 文件
lab/net.sh up && lab/net.sh selftest
lab/dhcp.sh start coexist    # 或 existing / rogue
lab/run.sh -j 2 B1 B2 …      # 场景编排，结果在 lab/out/<场景>/
lab/run.sh capture           # 把夹具复制到 crates/xpxe-dhcp/tests/fixtures/
lab/dhcp.sh stop && lab/net.sh down
```

完整验收命令与结论见 `docs/plan/step-02-boot-lab-spike.md`、`docs/lab/spike-report.md` 与 `docs/lab/compat-matrix.md`。

实验室工具（QEMU、OVMF、libvirt、sushy-tools、Kea、tcpdump）只存在于开发机，不进入产品镜像。`*.efi` / `*.kpxe` / 安装器内核与 initrd 不入库，由 `setup.sh` 从 Debian 包和镜像源取得。

## 网络与安全

- `br-lab`（10.77.0.1/24）不连接任何物理网卡、不做 NAT。
- `xpxe-lab` 中会发 DHCP 的子命令（`proxydhcp`、`dhcp-scan`、`pxe-client`）拒绝在没有 10.77.0.0/24 直连路由、或承载经实验网以外网关的默认路由的网卡上运行。
- QEMU 以非 root、经 `/dev/kvm` 的实际属组运行（`sg "$(stat -c %G /dev/kvm)"`）。
- `proxydhcp` / `tftp` 以 UID 10001 + 仅 `NET_BIND_SERVICE` 运行（验证内核 ≥ 5.7 时 `SO_BINDTODEVICE` 不需要 `NET_RAW`）。
