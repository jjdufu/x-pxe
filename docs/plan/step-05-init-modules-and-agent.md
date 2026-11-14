# Step 05：初始化模块与目标机执行器（`xpxe-domain::init` + `xpxe-render::init` + `xpxe-agent`）

## 1. 目标

把 `legacy/init-d13.sh` 的全部功能拆成有类型、有取值范围、有默认值的初始化模块，在 Web 上逐个编辑、实时预览、对比差异；服务端把选中的模块渲染成一份“初始化计划”，目标机首次启动时由静态 Rust 执行器 `xpxe-agent` 逐个执行并回报。

## 2. 目标机侧形态：Rust 执行器 + 渲染出的模块脚本

| 方案 | 优点 | 缺点 |
| :--- | :--- | :--- |
| 纯渲染 shell 脚本 | 最透明 | 超时、重试、结构化回报、失败时保证上报、断点续跑全要用 bash 写，难测 |
| 纯 Rust agent（模块逻辑写在 Rust 里） | 全类型化 | 模块本质是调用 `apt-get`/`systemctl`/`sysctl`，Rust 里仍是包一层 `Command`；Web 上看不到“到底会以 root 执行什么” |
| **Rust 执行器 + 渲染的模块脚本（采用）** | 生命周期（超时、重试、回报、续跑、自清理）由受门禁约束的 Rust 保证；每个模块的执行内容仍是可读的 bash，Web 预览与差异看到的就是实际执行的内容 | 目标机需要 bash（Debian 默认具备） |

`xpxe-agent` 构建为 `x86_64-unknown-linux-musl` 静态二进制（约 3 MB），由 `late_command` 下载并做 SHA-256 校验（Step 4）。

## 3. 初始化计划格式

```json
{
  "schema": 1,
  "session": "01J…",
  "events_url": "http://192.168.2.3:8080/p/<agent_token>/events",
  "log_path": "/var/log/xpxe-firstboot.log",
  "modules": [
    { "id": "apt_sources", "title": "APT 源整理", "timeout_s": 600, "continue_on_error": false,
      "script": "#!/bin/bash\nset -euo pipefail\n…", "sha256": "…" }
  ],
  "finalize": { "reboot": true }
}
```

## 4. 模块目录

模块顺序固定（依赖顺序），每个模块可单独启用。`来源` 列指向 `legacy/init-d13.sh` 中对应的功能段。内置两个计划预设：`k8s-node`（全部模块按 legacy 行为取值，等价性由第 7 节测试证明）与 `minimal`（只启用 `users_keys`、`sshd`、`timezone`、`time_sync`、`locale`、`apt_sources`、`packages.base_ops`）。

| # | 模块 id | 来源 | 字段（类型 / 范围 / 默认） | 生成物 |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `apt_sources` | APT 段 | `disable_cdrom: bool = true`；`uncomment_http: bool = true`；`fallback: Official{suites=[trixie, trixie-updates, trixie-backports], security=true, components=[main, contrib, non-free, non-free-firmware]} \| ProfileMirror \| None = Official`；`autoclean_before_update: bool = true` | 注释 `deb cdrom:` 行、禁用 cdrom 的 `.sources`、恢复被注释的 http(s) 行；无网络源时写 `/etc/apt/sources.list.d/debian.sources`（deb822，`Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg`）；`apt-get update` |
| 2 | `packages` | `PKGS` 数组 | 8 个分组开关（见 4.1）；`extra: Vec<PackageName>`；`exclude: Vec<PackageName>`；`install_recommends: bool = true`；`zram_tools: bool = false` | `apt-get install -y`，包名逐个经过 `PackageName` 校验 |
| 3 | `users_keys` | useradd 默认值、用户与公钥段 | `useradd_default_shell: /bin/bash \| /bin/sh = /bin/bash`；`root_authorized_keys: Vec<SshPublicKey>`；`users: Vec<{ name: UnixUser, shell, password: Locked \| Hash, groups: Vec<Group> = [sudo], authorized_keys, copy_root_keys: bool = true, linger: bool = true }>` | `/etc/default/useradd`、幂等创建用户、加组、`authorized_keys` 去重写入（`0600`，目录 `0700`，属主正确）、`loginctl enable-linger` |
| 4 | `timezone` | 时区段 | `zone: TimeZone = Asia/Shanghai`；`rtc_utc: bool = true` | `timedatectl set-timezone`、`set-local-rtc 0`、`/etc/timezone`、`/etc/localtime` |
| 5 | `time_sync` | timesyncd 段与 chrony 注释段 | `mode: Timesyncd{ servers: Vec<NtpServer> = [ntp.aliyun.com], ensure_installed: bool = true } \| Chrony{ pools: Vec<{ host, iburst: bool = true, maxsources: 1..=8 = 1 }> }` | `/etc/systemd/timesyncd.conf.d/ntp.conf` 并确保 `systemd-timesyncd` 已安装且 `enable --now`；或 chrony：注释默认 `pool`、写 `conf.d/xpxe.conf`、启用服务 |
| 6 | `locale` | locale 段 | `lang: Locale = en_US.UTF-8`；`lc_all: Option<Locale> = en_US.UTF-8`；`language: SafeText = en_US.UTF-8`；`generate: Vec<Locale> = [zh_CN.UTF-8, en_US.UTF-8]` | `/etc/default/locale`、`/etc/locale.gen` 去重启用、`locale-gen`、`update-locale` |
| 7 | `systemd_manager` | 启停超时段 | `default_timeout_start_s: 5..=600 = 30`；`default_timeout_stop_s: 5..=600 = 30` | `/etc/systemd/system.conf.d/timeout.conf` |
| 8 | `ssh_generator_fix` | VSOCK 刷屏段 | `mask_generator: bool = true`；`mask_sockets: Vec<UnitName> = [sshd-unix-local.socket, sshd-vsock.socket]` | `/etc/systemd/system-generators/systemd-ssh-generator → /dev/null`、`systemctl mask` |
| 9 | `sshd` | SSH drop-in 段与最终校验段 | `dropin_name: SafeFileName = jwczjzl.conf`；`permit_root_login: No \| ProhibitPassword \| Yes = No`；`password_authentication: bool = false`；`x11_forwarding = false`；`x11_use_localhost = true`；`allow_tcp_forwarding = true`；`allow_agent_forwarding = true`；`ignore_rhosts = true`；`gssapi_authentication = false`；`use_dns = false`；`compression = false`；`tcp_keepalive = true`；`banner: Option<AbsPath> = /etc/issue`；`sync_main_config: bool = true` | 整文件写 `/etc/ssh/sshd_config.d/<dropin_name>`；主配置同步 `X11Forwarding` 与 `Banner`；`sshd -t` 通过后 `reload`（失败时 `restart`） |
| 10 | `sudoers` | sudoers 与 logrotate 段 | `dropin_name: SafeFileName = jwczjzl`；`defaults: { env_reset = true, env_keep = [HOME], env_delete = [HTTP_PROXY, HTTPS_PROXY], passwd_tries: 1..=10 = 3, timestamp_timeout_min: 0..=60 = 10, secure_path, use_pty = true, logfile: AbsPath = /var/log/sudo.log }`；`nopasswd_alias: { name = SYSADMIN, commands: Vec<AbsPath> = [service, systemctl, apt-get, apt, reboot, shutdown 的绝对路径], groups = [%sudo] }`；`full_sudo_users: Vec<UnixUser> = [jwczjzl]`；`backup_original: bool = true`；`logrotate: { period: Daily \| Weekly \| Monthly = Weekly, rotate: 1..=52 = 4, compress = true }` | 先写临时文件并 `visudo -cf`，通过后再以 `0440` 安装到 `/etc/sudoers.d/`；再对 `/etc/sudoers` 整体 `visudo -cf`；`/etc/logrotate.d/sudo` |
| 11 | `shell_profile` | profile.d 段 | `color_aliases: bool = true`（ls/grep/fgrep/egrep 着色）；`short_aliases: bool = true`（ll/la/l）；`unset_mailcheck: bool = true`；`path_append: Vec<AbsPath> = [/usr/local/sbin]`；`ssh_agent: { enabled = true, key: RelPath = .ssh/id_ed25519 }` | `/etc/profile.d/zz-aliases.sh`、`/etc/profile.d/ssh-agent.sh`（`0644`） |
| 12 | `vimrc` | vimrc 段 | `enabled: bool = true`；`targets: Vec<UnixUser> = [root, jwczjzl]`；内容为内置模板 `vimrc.default`（与 legacy 内容一致） | 各用户 `~/.vimrc`，属主正确 |
| 13 | `services` | 服务段 | `disable_now: Vec<UnitName> = [apt-daily.timer, apt-daily-upgrade.timer]`；`enable_now: Vec<UnitName> = [ssh]`（启用前先 `unmask`）；`mask: Vec<UnitName> = [cryptdisks-early.service, cryptdisks.service, x11-common.service, nftables.service]`；`default_editor: Option<AbsPath> = /usr/bin/vim.basic` | `systemctl` 操作、`daemon-reload`、`update-alternatives --set editor` |
| 14 | `motd` | MOTD/Banner 段 | `clear_issue: bool = true`；`clear_motd: bool = true`；`disable_update_motd_scripts: bool = true`；`custom_header: { enabled = true, bar_width: 20..=120 = 60, warn_percent: 50..=99 = 90, exclude_fs = [zfs, squashfs, tmpfs, devtmpfs, overlay] }` | 清空 `/etc/issue(.net)`、`/etc/motd`；`chmod -x /etc/update-motd.d/*`；`/etc/update-motd.d/00-custom-header`（系统、内核、运行时长、CPU、内存、负载、进程、用户、磁盘条形图）；CPU 型号缺失（如 ARM64 无 `model name`）与非数字使用率在头部脚本内有兜底 |
| 15 | `sysctl` | 内核参数段 | `file_name: SafeFileName = 99-fleet-netvm.conf`；`preset: K8sNode \| WebServer \| Minimal = K8sNode`；约 40 个有类型参数（见 4.2）；`extra: Vec<{ key: SysctlKey, value: SafeText }>`；`apply_now: bool = true` | `/etc/sysctl.d/<file_name>`、`sysctl --system`（失败项作为警告回报，不中断） |
| 16 | `limits` | limits 段 | `file_name = 90-nofile.conf`；`nofile: 1024..=1048576 = 655350`（soft/hard）；`nproc: Unlimited \| N = Unlimited`；`core: Unlimited \| N = Unlimited` | `/etc/security/limits.d/<file_name>` |
| 17 | `history_audit` | 命令历史审计段 | `dir: AbsPath = /var/log/.history`；`dir_mode = 1777`；`histsize: 1000..=1000000 = 100000`；`append_only: bool = true`（`chattr +a`）；`timeformat_with_source_ip: bool = true` | `/etc/profile.d/history-record.sh`（每次登录独立 HISTFILE） |
| 18 | `fstrim` | fstrim 注释段 | `enable_timer: bool = false` | `systemctl enable --now fstrim.timer` |
| 19 | `custom_snippet` | （无，扩展入口） | `enabled: bool = false`；`snippets: Vec<{ name: SafeText, body: ShellText ≤ 64 KiB }>` | 见 4.3 |
| 20 | `cleanup` | Clean 段 | `remove_trust_files: bool = true`（`hosts.equiv`、`.netrc`、`.rhosts`，深度 3）；`apt_clean: bool = true`（clean/autoclean/autoremove）；`delete_apt_logs = true`；`delete_rotated_logs = true`；`truncate_logs = true`；`clear_tmp = true` | 清理动作；`/var/log/xpxe-firstboot.log` 永远不在清理范围内 |

计划级字段 `finalize.reboot: bool = true` 对应 legacy 末尾的 `reboot`。legacy 中仅以注释出现的选项按下表处理：

| legacy 注释项 | 处理 |
| :--- | :--- |
| chrony 配置 | `time_sync.mode = Chrony` |
| `zram-tools` | `packages.zram_tools` |
| fstrim 说明 | `fstrim` 模块 |
| `useradd -s /sbin/nologin` 服务账户 | `users_keys.users[].shell = /usr/sbin/nologin` 且 `password = Locked` |
| BBR | `sysctl.congestion = Bbr` |
| xanmod 内核 | 第三方仓库与内核，不做成模块；需要时用 Profile 的 `extra_repos` + `custom_snippet` |

### 4.1 软件包分组

8 个分组的并集正好等于 legacy `PKGS` 的 80 个包（第 7 节有测试保证）。

| 分组 | 默认（k8s-node / minimal） | 包 |
| :--- | :--- | :--- |
| `base_ops` 基础运维 | 开 / 开 | sudo vim git curl wget rsync jq tree tmux btop bash-completion man-db cron dialog dos2unix zip xz-utils p7zip-full psmisc ncdu ripgrep rsyslog iputils-ping gpg expect |
| `net_diag` 网络诊断 | 开 / 关 | dnsutils net-tools nload iftop tcptraceroute fping hping3 socat |
| `perf` 性能观测 | 开 / 关 | sysstat iotop nmon strace powertop |
| `storage` 存储与网络栈 | 开 / 关 | xfsprogs parted nfs-common rpcbind ifenslave libaio1t64 |
| `build` 开发构建 | 开 / 关 | gcc g++ make pkg-config libssl-dev liblzma-dev libncurses6 gh python3-defusedxml |
| `security` 安全 | 开 / 关 | fail2ban rkhunter clamav libpam-pwquality |
| `headless` 无头浏览器运行库 | 开 / 关 | libnspr4 libnss3 libatk1.0-0t64 libatk-bridge2.0-0t64 libatspi2.0-0t64 libcups2t64 libgbm1 libasound2t64 libxkbcommon0 libcairo2 libpango-1.0-0 libxcomposite1 libxdamage1 libxfixes3 libxrandr2 libvulkan1 xdg-utils |
| `docs_fonts` 文档与字体 | 开 / 关 | libreoffice-writer-nogui poppler-utils fonts-noto-cjk fonts-wqy-microhei fonts-liberation fonts-noto-color-emoji |

分组清单是版本化的内置数据（`data/package-groups.json`），升级 Debian 小版本时单独提交并附包存在性检查（Step 7 的镜像源 `Packages` 解析可复用）。

### 4.2 sysctl 参数

每个参数是一个有类型字段，三档预设只是不同的默认值组合。`K8sNode` 与 legacy 文件逐项一致：

| 组 | 参数（类型 / 范围 / K8sNode 默认） |
| :--- | :--- |
| 转发与 IPv6 | `net.ipv4.ip_forward` bool = 1；`disable_ipv6`（all/default/lo 三项联动）bool = 1，开启时 Web 显示“确认无双栈依赖”提示 |
| TCP | `tcp_syncookies` bool = 1；`tcp_tw_reuse` 0..=2 = 1；`tcp_fin_timeout` 5..=120 = 30；`tcp_syn_retries` 1..=10 = 5；`tcp_synack_retries` 1..=10 = 5；`tcp_fastopen` 0..=3 = 3；`tcp_slow_start_after_idle` bool = 0 |
| keepalive | `tcp_keepalive_time` 60..=7200 = 600；`tcp_keepalive_intvl` 10..=300 = 30；`tcp_keepalive_probes` 1..=20 = 5 |
| 缓冲 | `tcp_rmem` 三元组 = 4096 87380 12582912；`tcp_wmem` = 4096 16384 12582912；`rmem_default` = 212992；`wmem_default` = 16384；`rmem_max`/`wmem_max` = 12582912（校验 min ≤ default ≤ max） |
| 连接表 | `tcp_max_orphans` = 262144；`tcp_max_syn_backlog` = 32768；`tcp_max_tw_buckets` = 1048576；`ip_local_port_range` = 10000 65535（1024 ≤ lo < hi ≤ 65535） |
| 拥塞 | `congestion: SystemDefault \| Bbr(fq) \| Cubic(fq_codel) = SystemDefault` |
| 队列 | `netdev_max_backlog` = 32768；`somaxconn` = 32768 |
| 内存 | `vm.max_map_count` = 262144；`dirty_background_ratio` 1..=50 = 5（< `dirty_ratio`）；`dirty_ratio` 1..=90 = 10；`overcommit_memory` 0..=2 = 1；`swappiness` 0..=100 = 10；`min_free_kbytes` = 65536；`vfs_cache_pressure` 1..=1000 = 50 |
| 文件监控 | `fs.fanotify.max_queued_events` = 65536；`fs.inotify.max_user_watches` = 524288 |
| 安全 | `accept_redirects` bool = 0；`secure_redirects` bool = 0；`rp_filter` 0..=2 = 0 |

### 4.3 自定义片段（唯一的自由脚本入口）

- 默认关闭；只有 `admin` 角色能开启或修改，每次变更写审计（含正文 SHA-256）。
- 服务端用 `yash-syntax` 做 POSIX 语法解析，解析失败拒绝保存；bash 专有语法只给警告（目标机用 bash 执行）。
- 片段在 `cleanup` 之前执行，Web 上以危险色提示框标注“以 root 在每台目标机上执行”。

### 4.4 校验规则（跨模块）

- `sshd.permit_root_login = No` 且 `password_authentication = false` 时，至少一个非 root 用户必须同时具备 `sudo` 组与 SSH 公钥，否则拒绝保存（防止装完无法登录）。
- `services.mask` 包含 `nftables.service` 且启用了 `packages.security` 时给出警告（fail2ban 无防火墙后端）。
- `vimrc.targets`、`sudoers.full_sudo_users` 中的用户必须在 `users_keys.users` 或 Profile 的 `primary_user` 中存在。
- 所有文件名、单元名、路径都经过白名单类型，渲染时没有任何未经过滤的插值。

## 5. 渲染（`xpxe-render::init`）

- 每个模块一个 minijinja 模板，输出以 `#!/bin/bash` + `set -euo pipefail` 开头的独立脚本；所有写文件动作都是“整文件覆盖”，重复执行结果一致。
- `render_plan(plan, profile, session) -> RenderedPlan`，同时产出逐模块文本，供 Web 的“模块预览”和“整份计划预览”使用。
- 差异：同一计划两个版本的渲染结果做逐模块文本 diff，与配置层面的字段 diff（Step 3）一起展示。

## 6. `xpxe-agent`

```text
xpxe-agent run --plan /var/lib/xpxe/plan.json
xpxe-agent verify --plan <file>          # 只校验计划（排障用）
```

| 行为 | 说明 |
| :--- | :--- |
| 前置检查 | UID 0、计划 schema、每个模块 `sha256` |
| 回报 | `agent.started` → 每个模块 `module.started/finished`（进度观测，不触发状态转换）→ `agent.finished` 或 `agent.failed{ module, exit_code, log_tail: 最后 50 行 }` |
| 执行 | 每个模块以 `/bin/bash` 执行，独立超时；stdout/stderr 带模块前缀写入 `log_path` 与 journal |
| 续跑 | 完成的模块写 `/var/lib/xpxe/done/<id>.<sha256>`，意外重启后跳过已完成模块 |
| 网络重试 | 事件带 `event_id`，指数退避重试；最终事件在 `configure_ttl` 内持续重试，未送达前不重启 |
| 收尾 | 成功：`systemctl disable xpxe-firstboot.service`、删除 `plan.json`（含令牌）与 agent 自身、按 `finalize.reboot` 重启；失败：保留计划与日志，服务保持禁用，等待人工处理 |

## 7. 测试

| 测试 | 内容 |
| :--- | :--- |
| `xpxe-domain` 单元 | 每个字段的范围与默认值；跨模块校验；`package-groups.json` 并集 = legacy `PKGS`（测试从 `legacy/init-d13.sh` 解析 `PKGS=( … )` 对比，保证 legacy 变化时测试提醒） |
| `xpxe-render` 快照 | `k8s-node` 与 `minimal` 两个预设的逐模块快照；每个模块脚本经 `bash -n` |
| 注入 | 所有字符串字段写入 shell 元字符，断言被类型拒绝或被正确引用 |
| `xpxe-agent` 集成 | `tests/run/`：用临时目录与假的 `/bin/bash` 替身脚本验证超时、续跑、回报顺序、失败时不重启（重启动作经 trait 注入，测试实现只记录调用） |
| 等价性（实验室） | 两台全新 Debian 13 云镜像虚拟机：A 执行 `legacy/init-d13.sh`（替换口令与公钥为测试值），B 执行 `k8s-node` 预设计划；比较 `/etc/ssh/sshd_config.d/`、`/etc/sudoers.d/`、`/etc/sysctl.d/`、`/etc/security/limits.d/`、`/etc/profile.d/`、`/etc/update-motd.d/00-custom-header`、`/etc/default/locale`、`/etc/locale.gen`、`/etc/systemd/*.conf.d/`、`~/.vimrc`、`dpkg-query -W` 包清单、`systemctl list-unit-files --state=masked`；允许的差异只有 `/var/log/xpxe-firstboot.log` 被保留 |

## 8. 验收

```bash
cargo test -p xpxe-domain -p xpxe-render -p xpxe-agent --all-targets --locked
cargo build --locked --release --target x86_64-unknown-linux-musl -p xpxe-agent
file target/x86_64-unknown-linux-musl/release/xpxe-agent | grep -F 'statically linked'
cargo run -p xpxe-lab -- render-plan --preset k8s-node --out /tmp/plan.json
lab/run.sh init-equivalence --plan /tmp/plan.json --legacy legacy/init-d13.sh
scripts/gate.sh
```

验收标准：

- `init-equivalence` 报告除允许差异外零差异。
- 在 B 虚拟机上：`systemctl is-enabled xpxe-firstboot.service` 为 `disabled`，`/var/lib/xpxe/plan.json` 不存在，`/var/log/xpxe-firstboot.log` 存在且包含 20 个模块的起止记录（被禁用模块记录为 skipped）。
- `scripts/gate.sh` 全绿。
