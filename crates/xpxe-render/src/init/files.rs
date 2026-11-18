//! 由有类型设置拼出的配置文件正文（sysctl、sudoers、profile.d、sshd drop-in、APT 兜底源）。

use xpxe_domain::init::{
    AptFallback, Congestion, ShellProfile, Sshd, Sudoers, SysctlExtra, SysctlParams,
};
use xpxe_domain::{Profile, SecuritySource};

/// 布尔转 sysctl 的 `0/1`。
const fn bit(on: bool) -> u8 {
    if on { 1 } else { 0 }
}

/// 布尔转 sshd 的 `yes/no`。
const fn yes_no(on: bool) -> &'static str {
    if on { "yes" } else { "no" }
}

/// 输出缓冲：逐行收集，最后以换行连接。
type Lines = Vec<String>;

/// 追加静态文本（可含多行，末尾换行会被拆成空行语义）。
fn text(out: &mut Lines, raw: &str) {
    out.extend(raw.trim_end_matches('\n').split('\n').map(str::to_owned));
}

/// 写一行 `key = value`。
fn kv(out: &mut Lines, key: &str, value: impl std::fmt::Display) {
    out.push(format!("{key} = {value}"));
}

/// sysctl 文件正文（K8sNode 取值时与 legacy 文件逐字节一致）。
pub(crate) fn sysctl_content(params: &SysctlParams, extra: &[SysctlExtra]) -> String {
    let mut out: Lines = vec![String::new()];
    sysctl_forward_tcp(&mut out, params);
    sysctl_buffers(&mut out, params);
    sysctl_rest(&mut out, params);
    if !extra.is_empty() {
        text(&mut out, "\n# ------ 额外 ------");
        for item in extra {
            kv(&mut out, item.key.as_str(), item.value.as_str());
        }
    }
    out.push(String::new());
    out.join("\n")
}

/// 转发、IPv6、TCP 与 keepalive。
fn sysctl_forward_tcp(out: &mut Lines, params: &SysctlParams) {
    let fwd = &params.forward;
    text(
        out,
        "# ------ 转发 / IPv6 ------\n# K8s 节点需要转发；纯 nginx 开着也无妨\n",
    );
    kv(out, "net.ipv4.ip_forward", bit(fwd.ip_forward.is_on()));
    text(
        out,
        "# 确认集群无依赖 IPv6 后再禁用；若有双栈 Service 不要用这三项\n",
    );
    for scope in ["all", "default", "lo"] {
        kv(
            out,
            &format!("net.ipv6.conf.{scope}.disable_ipv6"),
            bit(fwd.disable_ipv6.is_on()),
        );
    }
    let tcp = &params.tcp;
    text(out, "\n# ------ TCP ------\n");
    kv(out, "net.ipv4.tcp_syncookies", bit(tcp.syncookies.is_on()));
    kv(out, "net.ipv4.tcp_tw_reuse", tcp.tw_reuse);
    kv(out, "net.ipv4.tcp_fin_timeout", tcp.fin_timeout);
    kv(out, "net.ipv4.tcp_syn_retries", tcp.syn_retries);
    kv(out, "net.ipv4.tcp_synack_retries", tcp.synack_retries);
    kv(out, "net.ipv4.tcp_fastopen", tcp.fastopen);
    kv(
        out,
        "net.ipv4.tcp_slow_start_after_idle",
        bit(tcp.slow_start_after_idle.is_on()),
    );
    let keep = &params.keepalive;
    text(
        out,
        "\n# 空闲 10 分钟后探活；配合 intvl/probes，死连接收得比 7200 快（适合 K8s/反代）\n",
    );
    kv(out, "net.ipv4.tcp_keepalive_time", keep.time);
    kv(out, "net.ipv4.tcp_keepalive_intvl", keep.intvl);
    kv(out, "net.ipv4.tcp_keepalive_probes", keep.probes);
}

/// 缓冲与连接表。
fn sysctl_buffers(out: &mut Lines, params: &SysctlParams) {
    let buf = &params.buffers;
    text(
        out,
        "\n# 缓冲：default 保持克制，max 允许大流量抬升（勿再把 default 调到数 MB）\n",
    );
    for (key, triple) in [
        ("net.ipv4.tcp_rmem", buf.tcp_rmem),
        ("net.ipv4.tcp_wmem", buf.tcp_wmem),
    ] {
        kv(
            out,
            key,
            format!("{} {} {}", triple.min, triple.default, triple.max),
        );
    }
    kv(out, "net.core.rmem_default", buf.rmem_default);
    kv(out, "net.core.wmem_default", buf.wmem_default);
    kv(out, "net.core.rmem_max", buf.rmem_max);
    kv(out, "net.core.wmem_max", buf.wmem_max);
    let conn = &params.conn_table;
    out.push(String::new());
    kv(out, "net.ipv4.tcp_max_orphans", conn.tcp_max_orphans);
    kv(
        out,
        "net.ipv4.tcp_max_syn_backlog",
        conn.tcp_max_syn_backlog,
    );
    kv(out, "net.ipv4.tcp_max_tw_buckets", conn.tcp_max_tw_buckets);
    let range = conn.ip_local_port_range;
    kv(
        out,
        "net.ipv4.ip_local_port_range",
        format!("{} {}", range.low, range.high),
    );
}

/// 拥塞、队列、内存、文件监控与安全。
fn sysctl_rest(out: &mut Lines, params: &SysctlParams) {
    text(
        out,
        match params.congestion {
            Congestion::SystemDefault => concat!(
                "\n# 拥塞：维持发行版常见组合即可；公网入口若要试 BBR，再单独开下面两行并回归测试\n",
                "# net.ipv4.tcp_congestion_control = bbr\n# net.core.default_qdisc = fq\n",
                "# 当前建议保持（若系统已是 cubic + fq_codel，不必强改）\n",
                "# net.ipv4.tcp_congestion_control = cubic\n# net.core.default_qdisc = fq_codel\n",
            ),
            Congestion::Bbr => {
                "\n# 拥塞：BBR\nnet.ipv4.tcp_congestion_control = bbr\nnet.core.default_qdisc = fq\n"
            }
            Congestion::Cubic => concat!(
                "\n# 拥塞：CUBIC\nnet.ipv4.tcp_congestion_control = cubic\n",
                "net.core.default_qdisc = fq_codel\n",
            ),
        },
    );
    text(out, "\n# ------ 队列 ------\n");
    kv(
        out,
        "net.core.netdev_max_backlog",
        params.queues.netdev_max_backlog,
    );
    kv(out, "net.core.somaxconn", params.queues.somaxconn);
    let mem = &params.memory;
    text(out, "\n# ------ 内存 / VM（ES/Java/K8s 友好）------\n");
    kv(out, "vm.max_map_count", mem.max_map_count);
    kv(out, "vm.dirty_background_ratio", mem.dirty_background_ratio);
    kv(out, "vm.dirty_ratio", mem.dirty_ratio);
    kv(out, "vm.overcommit_memory", mem.overcommit_memory);
    kv(out, "vm.swappiness", mem.swappiness);
    kv(out, "vm.min_free_kbytes", mem.min_free_kbytes);
    kv(out, "vm.vfs_cache_pressure", mem.vfs_cache_pressure);
    text(out, "\n# ------ 文件监控（kubelet / 同步类）------\n");
    kv(
        out,
        "fs.fanotify.max_queued_events",
        params.watch.fanotify_max_queued_events,
    );
    kv(
        out,
        "fs.inotify.max_user_watches",
        params.watch.inotify_max_user_watches,
    );
    let sec = &params.security;
    text(out, "\n# ------ 安全（与 Calico/K8s 兼容）------\n");
    for (name, value) in [
        ("accept_redirects", bit(sec.accept_redirects.is_on())),
        ("secure_redirects", bit(sec.secure_redirects.is_on())),
    ] {
        kv(out, &format!("net.ipv4.conf.all.{name}"), value);
        kv(out, &format!("net.ipv4.conf.default.{name}"), value);
    }
    text(
        out,
        "# 严格 rp_filter=1 易在覆盖网络/不对称路径上丢包；全员用 0（或改成 2 loose）\n",
    );
    kv(out, "net.ipv4.conf.all.rp_filter", sec.rp_filter);
    kv(out, "net.ipv4.conf.default.rp_filter", sec.rp_filter);
}

/// sshd drop-in 的各行（顺序与 legacy 一致）。
pub(crate) fn sshd_lines(sshd: &Sshd) -> Vec<String> {
    let mut lines = vec![
        format!("PermitRootLogin {}", sshd.permit_root_login.as_str()),
        format!("X11Forwarding {}", yes_no(sshd.x11_forwarding.is_on())),
        format!("X11UseLocalhost {}", yes_no(sshd.x11_use_localhost.is_on())),
        format!(
            "AllowTcpForwarding {}",
            yes_no(sshd.allow_tcp_forwarding.is_on())
        ),
        format!(
            "AllowAgentForwarding {}",
            yes_no(sshd.allow_agent_forwarding.is_on())
        ),
        format!("IgnoreRhosts {}", yes_no(sshd.ignore_rhosts.is_on())),
        format!(
            "GSSAPIAuthentication {}",
            yes_no(sshd.gssapi_authentication.is_on())
        ),
        format!("UseDNS {}", yes_no(sshd.use_dns.is_on())),
        format!("Compression {}", yes_no(sshd.compression.is_on())),
        format!("TCPKeepAlive {}", yes_no(sshd.tcp_keepalive.is_on())),
        format!(
            "PasswordAuthentication {}",
            yes_no(sshd.password_authentication.is_on())
        ),
    ];
    if let Some(banner) = &sshd.banner {
        lines.push(format!("Banner {banner}"));
    }
    lines
}

/// `x11_forwarding` 的 sshd 取值。
pub(crate) const fn x11_value(sshd: &Sshd) -> &'static str {
    yes_no(sshd.x11_forwarding.is_on())
}

/// 用分隔符连接。
fn join<T: std::fmt::Display>(items: &[T], sep: &str) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(sep)
}

/// sudoers drop-in 正文（默认取值时与 legacy 逐字节一致）。
pub(crate) fn sudoers_content(sudoers: &Sudoers) -> String {
    let def = &sudoers.defaults;
    let mut out: Lines = vec!["# 环境与审计（模块化；不替换整份 /etc/sudoers）".to_owned()];
    if def.env_reset.is_on() {
        out.push("Defaults    env_reset".to_owned());
    }
    if !def.env_keep.is_empty() {
        out.push(format!(
            "Defaults    env_keep += \"{}\"",
            join(&def.env_keep, " ")
        ));
    }
    if !def.env_delete.is_empty() {
        out.push(format!(
            "Defaults    env_delete = \"{}\"",
            join(&def.env_delete, " ")
        ));
    }
    out.push(format!("Defaults    passwd_tries={}", def.passwd_tries));
    out.push(format!(
        "Defaults    timestamp_timeout={}",
        def.timestamp_timeout_min
    ));
    if !def.secure_path.is_empty() {
        out.push(format!(
            "Defaults    secure_path=\"{}\"",
            join(&def.secure_path, ":")
        ));
    }
    if def.use_pty.is_on() {
        out.push("Defaults    use_pty".to_owned());
    }
    out.push(format!("Defaults    logfile=\"{}\"", def.logfile));
    text(
        &mut out,
        "\n# 常用运维命令免密；完整管理仍走 Debian 默认 %sudo ALL=(ALL:ALL) ALL（需密码）",
    );
    let alias = &sudoers.nopasswd_alias;
    if !alias.commands.is_empty() {
        out.push(format!(
            "Cmnd_Alias {} = {}",
            alias.name,
            join(&alias.commands, ", ")
        ));
        for group in &alias.groups {
            out.push(format!(
                "%{}   ALL=(ALL:ALL) NOPASSWD: {}",
                group.as_str(),
                alias.name
            ));
        }
    }
    for user in &sudoers.full_sudo_users {
        out.push(format!("{user} ALL=(ALL:ALL) ALL"));
    }
    out.join("\n")
}

/// `/etc/profile.d/zz-aliases.sh` 正文（默认取值时与 legacy 一致）。
pub(crate) fn aliases_content(profile: &ShellProfile) -> String {
    let mut out: Lines = Vec::new();
    if profile.unset_mailcheck.is_on() {
        text(&mut out, "unset MAILCHECK\n");
        out.push(String::new());
    }
    if profile.color_aliases.is_on() {
        text(
            &mut out,
            concat!(
                "if [ -x /usr/bin/dircolors ]; then\n",
                "    test -r ~/.dircolors && eval \"$(dircolors -b ~/.dircolors)\" || eval \"$(dircolors -b)\"\n",
                "    alias ls='ls --color=auto'\n",
                "    alias grep='grep --color=auto'\n",
                "    alias fgrep='fgrep --color=auto'\n",
                "    alias egrep='egrep --color=auto'\n",
                "fi",
            ),
        );
    }
    if profile.short_aliases.is_on() {
        text(
            &mut out,
            "alias ll='ls -alF'\nalias la='ls -A'\nalias l='ls -CF'",
        );
    }
    if !profile.path_append.is_empty() {
        out.push(String::new());
        out.push(format!(
            "export PATH=\"$PATH:{}\"",
            join(&profile.path_append, ":")
        ));
    }
    let joined = out.join("\n");
    let trimmed = joined.trim_end_matches('\n');
    if trimmed.is_empty() {
        "# x-pxe：未启用任何别名".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// deb822 段落。
fn deb822(uri: &str, suites: &str, comps: &str) -> String {
    format!(
        "Types: deb\nURIs: {uri}\nSuites: {suites}\nComponents: {comps}\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg"
    )
}

/// APT 兜底源（deb822）；`None` 表示不写。
pub(crate) fn apt_fallback(fallback: &AptFallback, profile: &Profile) -> Option<String> {
    match fallback {
        AptFallback::Off => None,
        AptFallback::Official {
            suites,
            security,
            components,
        } => {
            let comps = components
                .iter()
                .map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let suite_text = suites
                .iter()
                .map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let mut blocks = vec![deb822("https://deb.debian.org/debian", &suite_text, &comps)];
            if security.is_on() {
                blocks.push(deb822(
                    "https://deb.debian.org/debian-security",
                    "trixie-security",
                    &comps,
                ));
            }
            Some(blocks.join("\n\n"))
        }
        AptFallback::ProfileMirror => Some(profile_mirror(profile)),
    }
}

/// 由 Profile 镜像拼 deb822。
fn profile_mirror(profile: &Profile) -> String {
    let mirror = profile.mirror();
    let scheme = mirror.scheme.as_str();
    let comps = profile
        .components()
        .unique()
        .into_iter()
        .map(xpxe_domain::AptComponent::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    let mut suites = vec!["trixie"];
    if profile.suites().updates {
        suites.push("trixie-updates");
    }
    if profile.suites().backports {
        suites.push("trixie-backports");
    }
    let main_uri = format!("{scheme}://{}{}", mirror.host_port, mirror.directory);
    let mut blocks = vec![deb822(&main_uri, &suites.join(" "), &comps)];
    let security = match profile.security() {
        SecuritySource::Official => Some("http://security.debian.org/debian-security".to_owned()),
        SecuritySource::Mirror {
            host_port,
            directory,
        } => Some(format!("{scheme}://{host_port}{directory}")),
        SecuritySource::Disabled => None,
    };
    if let Some(uri) = security {
        blocks.push(deb822(&uri, "trixie-security", &comps));
    }
    blocks.join("\n\n")
}
