//! preseed.cfg 分段渲染。

use xpxe_domain::{
    ComponentSet, InstallAccounts, NodeNetwork, NodeSpec, PartitionScheme, RootAccount,
    SecuritySource, SuiteSet,
};

use crate::RenderError;
use crate::filters::sh_quote;
use crate::request::RenderRequest;

/// 渲染完整 preseed，段落之间空行分隔。
///
/// # Errors
///
/// 静态掩码转换失败时返回错误。
pub(crate) fn render_preseed(request: &RenderRequest) -> Result<String, RenderError> {
    let sections = [
        locale_section(request),
        network_section(request)?,
        mirror_section(request),
        apt_section(request),
        accounts_section(request.profile().accounts()),
        clock_section(request),
        partition_section(request.profile().partitioning()),
        software_section(request),
        bootloader_section(),
        finish_section(),
        hooks_section(request),
    ];
    let mut body = sections.join("\n\n");
    body.push('\n');
    Ok(body)
}

/// 本地化。
fn locale_section(request: &RenderRequest) -> String {
    format!(
        "d-i debian-installer/locale string {locale}\n\
d-i keyboard-configuration/xkb-keymap select {keymap}\n\
d-i console-setup/ask_detect boolean false",
        locale = request.profile().locale(),
        keymap = request.profile().keymap()
    )
}

/// 网络。
///
/// # Errors
///
/// 静态掩码失败时返回错误。
fn network_section(request: &RenderRequest) -> Result<String, RenderError> {
    let node = request.node();
    let mut lines = vec![
        format!(
            "d-i netcfg/choose_interface select {}",
            node.mac().to_colon()
        ),
        format!("d-i netcfg/get_hostname string {}", node.hostname()),
        format!("d-i netcfg/get_domain string {}", domain_of(node)),
        format!("d-i netcfg/hostname string {}", node.hostname()),
        "d-i hw-detect/load_firmware boolean true".to_owned(),
    ];
    push_static_preseed(&mut lines, node.network())?;
    Ok(lines.join("\n"))
}

/// 写入与内核参数一致的静态 netcfg。
///
/// # Errors
///
/// 掩码转换失败时返回错误。
fn push_static_preseed(lines: &mut Vec<String>, network: &NodeNetwork) -> Result<(), RenderError> {
    let NodeNetwork::Static {
        address,
        gateway,
        dns,
    } = network
    else {
        return Ok(());
    };
    lines.push("d-i netcfg/disable_autoconfig boolean true".to_owned());
    lines.push(format!(
        "d-i netcfg/get_ipaddress string {}",
        address.address()
    ));
    lines.push(format!(
        "d-i netcfg/get_netmask string {}",
        crate::cmdline::netmask_for(*address)?
    ));
    if let Some(gw_addr) = gateway {
        lines.push(format!("d-i netcfg/get_gateway string {gw_addr}"));
    }
    if !dns.is_empty() {
        let servers: Vec<String> = dns.iter().map(ToString::to_string).collect();
        lines.push(format!(
            "d-i netcfg/get_nameservers string {}",
            servers.join(" ")
        ));
    }
    lines.push("d-i netcfg/confirm_static boolean true".to_owned());
    Ok(())
}

/// 节点域名。
fn domain_of(node: &NodeSpec) -> String {
    crate::cmdline::domain_of(node)
}

/// 镜像源。
fn mirror_section(request: &RenderRequest) -> String {
    let mirror = request.profile().mirror();
    let proxy = mirror
        .proxy
        .as_ref()
        .map_or_else(String::new, |url| url.as_str().to_owned());
    format!(
        "d-i mirror/country string manual\n\
d-i mirror/protocol string {scheme}\n\
d-i mirror/http/hostname string {host}\n\
d-i mirror/http/directory string {dir}\n\
d-i mirror/http/proxy string {proxy}\n\
d-i mirror/suite string trixie\n\
d-i mirror/udeb/suite string trixie",
        scheme = mirror.scheme.as_str(),
        host = mirror.host_port.as_str(),
        dir = mirror.directory.as_str()
    )
}

/// APT 服务与组件。
fn apt_section(request: &RenderRequest) -> String {
    let profile = request.profile();
    let mut lines = vec![format!(
        "d-i apt-setup/services-select multiselect {}",
        services_select(profile.security(), profile.suites())
    )];
    push_security(&mut lines, profile.security());
    push_components(&mut lines, profile.components());
    push_extra_repos(&mut lines, request);
    lines.join("\n")
}

/// `services-select` 列表。
fn services_select(security: &SecuritySource, suites: SuiteSet) -> String {
    let mut items = Vec::new();
    if !matches!(security, SecuritySource::Disabled) {
        items.push("security");
    }
    if suites.updates {
        items.push("updates");
    }
    if suites.backports {
        items.push("backports");
    }
    items.join(",")
}

/// 安全更新主机。
fn push_security(lines: &mut Vec<String>, security: &SecuritySource) {
    match security {
        SecuritySource::Official => {
            lines.push("d-i apt-setup/security_host string security.debian.org".to_owned());
            lines.push("d-i apt-setup/security_path string /debian-security".to_owned());
        }
        SecuritySource::Mirror {
            host_port,
            directory,
        } => {
            lines.push(format!(
                "d-i apt-setup/security_host string {}",
                host_port.as_str()
            ));
            lines.push(format!(
                "d-i apt-setup/security_path string {}",
                directory.as_str()
            ));
        }
        SecuritySource::Disabled => {
            lines.push("d-i apt-setup/security_host string".to_owned());
            lines.push("d-i apt-setup/security_path string".to_owned());
        }
    }
}

/// 组件开关。
fn push_components(lines: &mut Vec<String>, components: &ComponentSet) {
    let flag = |present: bool| if present { "true" } else { "false" };
    lines.push(format!(
        "d-i apt-setup/non-free-firmware boolean {}",
        flag(components.contains(xpxe_domain::AptComponent::NonFreeFirmware))
    ));
    lines.push(format!(
        "d-i apt-setup/contrib boolean {}",
        flag(components.contains(xpxe_domain::AptComponent::Contrib))
    ));
    lines.push(format!(
        "d-i apt-setup/non-free boolean {}",
        flag(components.contains(xpxe_domain::AptComponent::NonFree))
    ));
}

/// 额外仓库与公钥 URL。
fn push_extra_repos(lines: &mut Vec<String>, request: &RenderRequest) {
    for (index, repo) in request.profile().extra_repos().iter().enumerate() {
        let number = index.saturating_add(1);
        let comps: Vec<&str> = repo
            .components
            .unique()
            .iter()
            .map(|item| item.as_str())
            .collect();
        lines.push(format!(
            "d-i apt-setup/local{number}/repository string {} {} {}",
            repo.url.as_str(),
            repo.suite.as_str(),
            comps.join(" ")
        ));
        lines.push(format!(
            "d-i apt-setup/local{number}/key string {}/repo-key/{index}.asc",
            request.session_base()
        ));
    }
}

/// 账户。
fn accounts_section(accounts: &InstallAccounts) -> String {
    match &accounts.root {
        RootAccount::Disabled => disabled_root(accounts),
        RootAccount::Hash { hash } => enabled_root(accounts, hash.as_str()),
    }
}

/// 禁用 root。
fn disabled_root(accounts: &InstallAccounts) -> String {
    let Some(user) = &accounts.primary_user else {
        return "d-i passwd/root-login boolean false".to_owned();
    };
    let groups: Vec<&str> = user
        .groups
        .iter()
        .map(xpxe_domain::UnixGroup::as_str)
        .collect();
    format!(
        "d-i passwd/root-login boolean false\n\
d-i passwd/make-user boolean true\n\
d-i passwd/user-fullname string {full}\n\
d-i passwd/username string {name}\n\
d-i passwd/user-password-crypted password {hash}\n\
d-i passwd/user-default-groups string {groups}",
        full = user.full_name.as_str(),
        name = user.name.as_str(),
        hash = user.hash.as_str(),
        groups = groups.join(" ")
    )
}

/// 启用 root。
fn enabled_root(accounts: &InstallAccounts, hash: &str) -> String {
    let mut lines = vec![
        "d-i passwd/root-login boolean true".to_owned(),
        format!("d-i passwd/root-password-crypted password {hash}"),
    ];
    if let Some(user) = &accounts.primary_user {
        let groups: Vec<&str> = user
            .groups
            .iter()
            .map(xpxe_domain::UnixGroup::as_str)
            .collect();
        lines.push("d-i passwd/make-user boolean true".to_owned());
        lines.push(format!(
            "d-i passwd/user-fullname string {}",
            user.full_name.as_str()
        ));
        lines.push(format!("d-i passwd/username string {}", user.name.as_str()));
        lines.push(format!(
            "d-i passwd/user-password-crypted password {}",
            user.hash.as_str()
        ));
        lines.push(format!(
            "d-i passwd/user-default-groups string {}",
            groups.join(" ")
        ));
    } else {
        lines.push("d-i passwd/make-user boolean false".to_owned());
    }
    lines.join("\n")
}

/// 时钟与 NTP。
fn clock_section(request: &RenderRequest) -> String {
    let ntp = request.profile().ntp();
    if ntp.is_empty() {
        format!(
            "d-i clock-setup/utc boolean true\n\
d-i time/zone string {}\n\
d-i clock-setup/ntp boolean false",
            request.profile().timezone()
        )
    } else {
        let servers: Vec<&str> = ntp.iter().map(xpxe_domain::NtpServer::as_str).collect();
        format!(
            "d-i clock-setup/utc boolean true\n\
d-i time/zone string {}\n\
d-i clock-setup/ntp boolean true\n\
d-i clock-setup/ntp-server string {}",
            request.profile().timezone(),
            servers.join(" ")
        )
    }
}

/// 分区方案。
fn partition_section(scheme: &PartitionScheme) -> String {
    match scheme {
        PartitionScheme::AtomicLvm => ATOMIC_LVM.to_owned(),
        PartitionScheme::AtomicPlain => ATOMIC_PLAIN.to_owned(),
        PartitionScheme::Custom { partitions } => {
            custom_preseed(partitions.iter().any(|part| part.on_lvm))
        }
    }
}

/// LVM atomic。
const ATOMIC_LVM: &str = "\
d-i partman-auto/method string lvm\n\
d-i partman-lvm/device_remove_lvm boolean true\n\
d-i partman-md/device_remove_md boolean true\n\
d-i partman-lvm/confirm boolean true\n\
d-i partman-lvm/confirm_nooverwrite boolean true\n\
d-i partman-auto/choose_recipe select atomic\n\
d-i partman-auto-lvm/guided_size string max\n\
d-i partman-partitioning/confirm_write_new_label boolean true\n\
d-i partman/choose_partition select finish\n\
d-i partman/confirm boolean true\n\
d-i partman/confirm_nooverwrite boolean true";

/// 非 LVM atomic。
const ATOMIC_PLAIN: &str = "\
d-i partman-auto/method string regular\n\
d-i partman-auto/choose_recipe select atomic\n\
d-i partman-partitioning/confirm_write_new_label boolean true\n\
d-i partman/choose_partition select finish\n\
d-i partman/confirm boolean true\n\
d-i partman/confirm_nooverwrite boolean true";

/// 自定义：方法由是否使用 LVM 决定，配方由 partman early 写入。
fn custom_preseed(on_lvm: bool) -> String {
    let method = if on_lvm { "lvm" } else { "regular" };
    format!(
        "d-i partman-auto/method string {method}\n\
d-i partman-lvm/device_remove_lvm boolean true\n\
d-i partman-md/device_remove_md boolean true\n\
d-i partman-lvm/confirm boolean true\n\
d-i partman-lvm/confirm_nooverwrite boolean true\n\
d-i partman-auto/choose_recipe select atomic\n\
d-i partman-partitioning/confirm_write_new_label boolean true\n\
d-i partman/choose_partition select finish\n\
d-i partman/confirm boolean true\n\
d-i partman/confirm_nooverwrite boolean true"
    )
}

/// 软件。
fn software_section(request: &RenderRequest) -> String {
    let tasks: Vec<&str> = request
        .profile()
        .tasks()
        .unique()
        .iter()
        .map(|task| task.as_str())
        .collect();
    format!(
        "d-i base-installer/kernel/image string linux-image-amd64\n\
tasksel tasksel/first multiselect {}\n\
d-i pkgsel/upgrade select {}\n\
popularity-contest popularity-contest/participate boolean false",
        tasks.join(", "),
        request.profile().pkgsel_upgrade().as_preseed()
    )
}

/// 引导器；bootdev 由选盘脚本写入，这里给默认占位。
fn bootloader_section() -> String {
    "d-i grub-installer/only_debian boolean true\n\
d-i grub-installer/bootdev string default"
        .to_owned()
}

/// 结束。
fn finish_section() -> String {
    "d-i finish-install/reboot_in_progress note\n\
d-i debian-installer/exit/poweroff boolean false"
        .to_owned()
}

/// 钩子：early / partman / late。
fn hooks_section(request: &RenderRequest) -> String {
    let early = format!(
        "wget -q -O /dev/null --post-data {payload} {url} || true",
        payload = sh_quote(&format!(
            "event=installer.started event_id={}",
            request.event_id().as_str()
        )),
        url = sh_quote(&format!(
            "{}/events/installer.started",
            request.session_base()
        ))
    );
    let partman = format!(
        "wget -q -O /tmp/xpxe-partman-early.sh {url} && sh /tmp/xpxe-partman-early.sh",
        url = sh_quote(&format!("{}/partman-early.sh", request.session_base()))
    );
    let late = format!(
        "wget -q -O /tmp/xpxe-late.sh {url} && sh /tmp/xpxe-late.sh",
        url = sh_quote(&format!("{}/late.sh", request.session_base()))
    );
    format!(
        "d-i preseed/early_command string {early}\n\
d-i partman/early_command string {partman}\n\
d-i preseed/late_command string {late}"
    )
}
