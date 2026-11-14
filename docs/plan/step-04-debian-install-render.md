# Step 04：Debian 13 安装渲染（`xpxe-domain::profile` + `xpxe-render`）

## 1. 目标

把“装机 Profile + 节点规格 + 会话”渲染成目标机实际拿到的四样东西：iPXE 脚本、内核参数、`preseed.cfg`、d-i 内运行的选盘与回调脚本。渲染是纯函数，相同输入得到逐字节相同的输出，便于预览、差异对比和快照测试。

## 2. 交付物

| 路径 | 内容 |
| :--- | :--- |
| `crates/xpxe-domain/src/profile.rs` | `Profile` 文档类型（实现 `ConfigDocument`）与校验 |
| `crates/xpxe-render/src/` | `ipxe.rs`、`cmdline.rs`、`preseed.rs`、`disk.rs`、`filters.rs`、`templates/*.j2`（`include_str!` 编入） |
| `crates/xpxe-render/tests/render/` | 目录形式集成测试与快照文件 `golden/*.txt` |
| `crates/xpxe-render/tests/disk_select/` | 在伪造 sysfs 树上执行选盘脚本的测试 |
| `crates/xpxe-domain/data/presets/` | 内置 Profile 预设：`nexus-lvm.json`（与 `legacy/pxe-server.sh` 的装机参数一致，口令除外）、`official-mirror.json` |

## 3. Profile 文档

```rust
pub struct Profile {
    pub name: SafeText,
    pub locale: Locale,                 // 默认 en_US.UTF-8
    pub keymap: Keymap,                 // 默认 us
    pub timezone: TimeZone,             // 默认 Asia/Shanghai
    pub ntp: Vec<NtpServer>,            // d-i 安装期 NTP，可空
    pub mirror: Mirror,                 // { scheme, host_port, directory, proxy: Option<HttpUrl> }
    pub security: SecuritySource,       // Official | Mirror { host_port, directory } | Disabled
    pub suites: SuiteSet,               // updates / backports 开关
    pub components: ComponentSet,       // main contrib non-free non-free-firmware
    pub extra_repos: Vec<AptRepo>,      // URL、suite、components、OpenPGP 公钥（校验后显示指纹）
    pub accounts: InstallAccounts,      // root: Disabled | Hash(CryptHash)；primary_user: { name, full_name, hash, groups }
    pub tasks: TaskSet,                 // 默认 ssh-server、standard
    pub pkgsel_upgrade: Upgrade,        // None | SafeUpgrade | FullUpgrade
    pub disk: DiskRule,
    pub partitioning: PartitionScheme,
    pub console: ConsoleSet,            // tty0、ttyS0,115200n8 等
    pub firmware_cpio: bool,            // 追加 Debian firmware.cpio.gz（需对应资产已激活）
    pub debug: InstallerDebug,          // Off | Debconf(1..=5)
}
```

- 口令：Web 只提交一次明文，服务端立即用 `sha-crypt` 生成 `$6$rounds=656000$…` 哈希后丢弃明文；也接受粘贴现成的 `$6$`/`$y$` 哈希。哈希字段在差异里只显示“已更改”。
- 校验示例：`Disabled` 根账户时必须有 `primary_user` 且其组含 `sudo`；`extra_repos` 的公钥必须能被 OpenPGP 公钥解析器（`xpxe-assets`）解析；`security = Official` 与 `XPXE_OFFLINE=true` 同时出现时给出警告。

## 4. 渲染输出

### 4.1 iPXE 脚本

会话脚本 `/p/{token}/boot.ipxe`：

```ipxe
#!ipxe
set base {{ boot_base }}/p/{{ boot_token }}
echo x-pxe: installing {{ hostname }} ({{ mac }}) session {{ session_short }}
kernel ${base}/linux initrd=initrd.gz {{ cmdline }}
initrd ${base}/initrd.gz
{% if firmware_cpio %}initrd ${base}/firmware.cpio.gz
{% endif %}boot || goto failed
:failed
echo x-pxe: boot failed, falling back to local disk in 10 s
sleep 10
exit
```

非 `Armed` 入口脚本固定为“打印原因 → `sleep 3` → `exit`”。失败分支一律 `exit`，避免重启后形成循环。`initrd=initrd.gz` 是 UEFI 下 EFI stub 找到 initrd 的必要参数，BIOS 下无副作用。

### 4.2 内核参数

由 `CmdlineBuilder` 组装，参数有序、去重，总长 ≤ 2048 字节（x86 `COMMAND_LINE_SIZE`），超长直接报错：

| 参数 | 值 |
| :--- | :--- |
| `auto=true priority=critical` | 固定 |
| `preseed/url` | `{{ boot_base }}/p/{{ boot_token }}/preseed.cfg` |
| `preseed/url/checksum` | 本次渲染的 preseed 的 MD5（d-i 只支持 MD5，用作完整性而非保密） |
| `BOOTIF` | `01-{{ mac:hexhyp }}`，多网卡时让 netcfg 选中引导网卡（Step 2 H7） |
| `hostname`、`domain` | 节点规格 |
| `locale`、`keymap` | Profile |
| 静态 IP | `netcfg/disable_autoconfig=true netcfg/get_ipaddress=… netcfg/get_netmask=… netcfg/get_gateway=… netcfg/get_nameservers=… netcfg/confirm_static=true`。静态地址必须在内核参数里给出，因为 d-i 在下载 preseed 之前就要完成网络配置 |
| `console=` | Profile |
| `DEBCONF_DEBUG` | 仅调试时 |

### 4.3 preseed.cfg

按段落渲染，每段对应一个模板片段，便于 Web 分段预览：

| 段 | 关键条目 |
| :--- | :--- |
| 本地化 | `debian-installer/locale`、`keyboard-configuration/xkb-keymap`、`console-setup/ask_detect false` |
| 网络 | `netcfg/get_hostname`、`netcfg/get_domain`、`netcfg/hostname`、`hw-detect/load_firmware true`；静态时重复写入与内核参数一致的 `netcfg/*` |
| 镜像源 | `mirror/country manual`、`mirror/protocol`、`mirror/http/hostname`、`mirror/http/directory`、`mirror/http/proxy`、`mirror/suite trixie`、`mirror/udeb/suite trixie` |
| APT | `apt-setup/services-select`（按 security/updates/backports）、`apt-setup/security_host`/`security_path`、`apt-setup/non-free-firmware`、`apt-setup/contrib`、`apt-setup/non-free`、`apt-setup/localN/*`（含 `key` URL，指向 `/p/{token}/repo-key/N.asc`） |
| 账户 | `passwd/root-login`、`passwd/root-password-crypted`、`passwd/make-user`、`passwd/user-fullname`、`passwd/username`、`passwd/user-password-crypted`、`passwd/user-default-groups` |
| 时钟 | `clock-setup/utc true`、`time/zone`、`clock-setup/ntp`、`clock-setup/ntp-server` |
| 分区 | 见 4.5 |
| 软件 | `base-installer/kernel/image linux-image-amd64`、`tasksel/first`、`pkgsel/upgrade`、`popularity-contest/participate false` |
| 引导器 | `grub-installer/only_debian true`、`grub-installer/bootdev`（由选盘脚本写入） |
| 结束 | `finish-install/reboot_in_progress note`、`debian-installer/exit/poweroff false` |
| 钩子 | `preseed/early_command`、`partman/early_command`、`preseed/late_command`（见 4.4） |

### 4.4 d-i 钩子与回调

所有钩子由模板生成，变量统一经过 `sh_quote` 过滤器（单引号包裹并转义），模板内不允许裸插值：

| 钩子 | 动作 |
| :--- | :--- |
| `preseed/early_command` | `wget --post-data` 上报 `installer.started`（`event_id` 为渲染时生成的 UUID，重试安全） |
| `partman/early_command` | 运行选盘脚本（4.5），成功则 `debconf-set partman-auto/disk` 与 `grub-installer/bootdev`，并按 `/sys/firmware/efi` 是否存在选择 UEFI 或 BIOS 分区配方；失败则上报 `installer.failed{reason: disk_selection, inventory}` 并以非零退出，d-i 停在错误界面，不猜盘 |
| `preseed/late_command` | 下载 `/p/{token}/agent` 到 `/target/usr/local/sbin/xpxe-agent` 并用 `sha256sum -c` 校验；下载 `/p/{agent_token}/plan.json` 到 `/target/var/lib/xpxe/plan.json`（`0600`）并校验；写入 `xpxe-firstboot.service` 并 `in-target systemctl enable`；上报 `installer.finished` |

`xpxe-firstboot.service`：

```ini
[Unit]
Description=x-pxe first boot configuration
Wants=network-online.target
After=network-online.target
ConditionPathExists=/var/lib/xpxe/plan.json

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/xpxe-agent run --plan /var/lib/xpxe/plan.json
StandardOutput=journal+console
StandardError=journal+console

[Install]
WantedBy=multi-user.target
```

### 4.5 确定性选盘与分区

```rust
pub struct DiskRule {
    pub include: Vec<DiskMatch>,         // ById(glob) | Wwn | Serial | Model(regex) | Transport(Nvme|Sata|Sas|Virtio|Scsi)
    pub size_gib: SizeRange,             // 默认 min 16
    pub exclude_models: Vec<SafeText>,   // 默认含 BMC 虚拟介质特征：Virtual、iDRAC、iLO、AMI、Cypress、Linux File-Stor、RMM、Virtual CDROM、Virtual Floppy
    pub prefer: Prefer,                  // Unique（必须恰好一块）| Smallest | Largest
}
```

选盘脚本（POSIX sh，d-i busybox 可执行）固定排除：`removable=1`、`ro=1`、设备路径含 `/usb`、`loop|ram|sr|fd|dm-|md|zram` 前缀、容量为 0、命中 `exclude_models` 的 vendor/model。剩余磁盘按 `include` 与容量过滤，再按 `prefer` 选择；`Unique` 下匹配数不为 1 即失败。脚本从 `${XPXE_SYSFS:-/sys}` 读取信息，测试时指向伪造的 sysfs 树。失败时上报的 `inventory` 列出每块盘的名称、容量、传输类型、vendor/model/serial 与被排除原因，Web 节点详情直接展示。

分区方案：

```rust
pub enum PartitionScheme { AtomicLvm, AtomicPlain, Custom(Vec<PartitionSpec>) }
pub struct PartitionSpec { mount: MountPoint, size: SizeSpec, fs: FsType, on_lvm: bool }
```

- `AtomicLvm` 对应 `legacy/pxe-server.sh` 的 `lvm + atomic + guided_size max`，同时写入 `partman-lvm/device_remove_lvm`、`partman-md/device_remove_md`、`partman-lvm/confirm(_nooverwrite)` 与 `partman/confirm(_nooverwrite)`。
- `Custom` 渲染为 `partman-auto/expert_recipe`，UEFI 配方自动加 512 MiB ESP，BIOS 配方自动加 1 MiB `biosgrub`，标签统一 `gpt`。
- 校验：恰好一个 `/`；`Rest` 至多一个；swap 至多一个；`/boot` 不在 LVM 上。

## 5. 测试

| 测试 | 内容 |
| :--- | :--- |
| `tests/render/` 快照 | 预设 × {DHCP, 静态} × {AtomicLvm, Custom} × {firmware_cpio 开/关} 的 iPXE、cmdline、preseed 快照 |
| 注入防护 | 每个字符串字段写入 `'`、`$(id)`、反引号、换行、`;`，渲染失败或结果被正确转义 |
| 长度 | cmdline 2048 字节边界 |
| 一致性 | 内核参数里的静态地址与 preseed 中的 `netcfg/*` 一致；MD5 与 preseed 正文一致 |
| `tests/disk_select/` | 伪造 sysfs：单 NVMe、NVMe + BMC 虚拟 U 盘、两块同型号 SATA（`Unique` 失败）、全部小于下限、by-id 精确匹配；在 `/bin/sh` 下执行并断言输出与退出码；同时执行 `sh -n` |
| 预设 | `nexus-lvm.json` 渲染结果与 `legacy/pxe-server.sh` 中的 preseed 条目逐项对照：mirror 主机与目录、suite 为 trixie、无 security、时区、NTP、LVM atomic、tasksel；口令必须来自输入 |

## 6. 验收

```bash
cargo test -p xpxe-render --all-targets --locked
cargo run -p xpxe-lab -- render --preset nexus-lvm --node lab/nodes/vm-uefi.json --out /tmp/r   # 输出 boot.ipxe、cmdline.txt、preseed.cfg
lab/run.sh install-uefi --www /tmp/r    # 用 Step 2 实验室装一台 OVMF 虚拟机直到 d-i 完成
lab/run.sh install-bios --www /tmp/r
lab/run.sh install-static --www /tmp/r  # 隔离网段，无 DHCP
scripts/gate.sh
```

验收标准：

- 三个实验室安装场景都完成 d-i 并重启，日志中可见 `installer.started` 与 `installer.finished` 两次 POST（实验室 HTTP 端记录）。
- 静态场景装好的系统 `/etc/network/interfaces` 为静态配置，地址与节点规格一致。
- 带 BMC 虚拟 U 盘的虚拟机（QEMU 附加 `usb-storage`）选中的是系统盘。
- `scripts/gate.sh` 全绿。
