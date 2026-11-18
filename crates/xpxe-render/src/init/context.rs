//! 每个模块的模板上下文：只放已校验的有类型值或由它们拼出的文本。

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use xpxe_domain::init::{
    LimitValue, ModuleId, PackageCatalog, PlainPath, TargetPlan, TimeSyncMode, UserPassword,
};
use xpxe_domain::{Locale, Profile};

use super::files::{
    aliases_content, apt_fallback, sshd_lines, sudoers_content, sysctl_content, x11_value,
};
use super::{render_template, sha256_hex};
use crate::RenderError;

/// 内置 vimrc（与 legacy 内容一致）。
const VIMRC: &str = include_str!("../templates/init/vimrc.default");
/// 登录头部脚本模板。
const MOTD_HEADER: &str = include_str!("../templates/init/motd-header.sh.j2");

/// 构造某模块的上下文。
///
/// # Errors
///
/// 分组数据损坏或头部模板失败时返回错误。
pub(super) fn build(
    id: ModuleId,
    plan: &TargetPlan,
    profile: &Profile,
    log_path: &PlainPath,
) -> Result<Value, RenderError> {
    let modules = &plan.modules;
    Ok(match id {
        ModuleId::AptSources => apt_sources(plan, profile),
        ModuleId::Packages => packages(plan)?,
        ModuleId::UsersKeys => users(plan),
        ModuleId::Timezone => {
            let tz = &modules.timezone.settings;
            json!({ "zone": tz.zone.as_str(), "local_rtc": u8::from(!tz.rtc_utc.is_on()) })
        }
        ModuleId::TimeSync => time_sync(&modules.time_sync.settings.mode),
        ModuleId::Locale => locale(plan),
        ModuleId::SystemdManager => {
            let mgr = &modules.systemd_manager.settings;
            json!({ "start": mgr.default_timeout_start_s.get(), "stop": mgr.default_timeout_stop_s.get() })
        }
        ModuleId::SshGeneratorFix => {
            let fix = &modules.ssh_generator_fix.settings;
            json!({ "mask_generator": fix.mask_generator.is_on(), "sockets": fix.mask_sockets })
        }
        ModuleId::Sshd => sshd(plan),
        ModuleId::Sudoers => sudoers(plan),
        ModuleId::ShellProfile => shell_profile(plan),
        ModuleId::Vimrc => json!({
            "targets": modules.vimrc.settings.targets,
            "vimrc": VIMRC.trim_end_matches('\n'),
        }),
        ModuleId::Services => services(plan),
        ModuleId::Motd => motd(plan)?,
        ModuleId::Sysctl => sysctl(plan),
        ModuleId::Limits => limits(plan),
        ModuleId::HistoryAudit => history(plan),
        ModuleId::Fstrim => json!({ "enable_timer": modules.fstrim.settings.enable_timer.is_on() }),
        ModuleId::CustomSnippet => snippets(plan),
        ModuleId::Cleanup => cleanup(plan, log_path),
    })
}

/// 软件包。
///
/// # Errors
///
/// 分组数据损坏时返回错误。
fn packages(plan: &TargetPlan) -> Result<Value, RenderError> {
    let pkgs = &plan.modules.packages.settings;
    let catalog = PackageCatalog::builtin()
        .map_err(|error| RenderError::new("package_catalog", error.to_string()))?;
    let list = pkgs
        .resolve(&catalog)
        .map_err(|error| RenderError::new("package_catalog", error.to_string()))?;
    Ok(json!({ "packages": list, "recommends": pkgs.install_recommends.is_on() }))
}

/// 用户与公钥。
fn users(plan: &TargetPlan) -> Value {
    let keys = &plan.modules.users_keys.settings;
    let users: Vec<Value> = keys
        .users
        .iter()
        .map(|user| {
            let hash = match &user.password {
                UserPassword::Locked => None,
                UserPassword::Hash { hash } => Some(hash.as_str()),
            };
            let groups: Vec<&str> = user
                .groups
                .iter()
                .map(xpxe_domain::UnixGroup::as_str)
                .collect();
            json!({
                "name": user.name.as_str(),
                "shell": user.shell.as_str(),
                "hash": hash,
                "groups": (!groups.is_empty()).then(|| groups.join(",")),
                "keys": user.authorized_keys,
                "copy_root_keys": user.copy_root_keys.is_on(),
                "linger": user.linger.is_on(),
            })
        })
        .collect();
    json!({
        "default_shell": keys.useradd_default_shell.as_str(),
        "root_keys": keys.root_authorized_keys,
        "users": users,
    })
}

/// 时间同步。
fn time_sync(mode: &TimeSyncMode) -> Value {
    match mode {
        TimeSyncMode::Timesyncd {
            servers,
            ensure_installed,
        } => {
            let list: Vec<&str> = servers.iter().map(xpxe_domain::NtpServer::as_str).collect();
            json!({
                "mode": "timesyncd",
                "servers": list.join(" "),
                "ensure_installed": ensure_installed.is_on(),
            })
        }
        TimeSyncMode::Chrony { pools } => {
            let lines: Vec<String> = pools
                .iter()
                .map(|pool| {
                    let burst = if pool.iburst.is_on() { " iburst" } else { "" };
                    format!("pool {}{burst} maxsources {}", pool.host, pool.maxsources)
                })
                .collect();
            json!({ "mode": "chrony", "pools": lines })
        }
    }
}

/// `locale.gen` 中的字符集。
fn charset(locale: &Locale) -> &'static str {
    let raw = locale.as_str();
    if raw.contains(".UTF-8") || raw.contains(".utf8") {
        "UTF-8"
    } else if raw.ends_with("@euro") {
        "ISO-8859-15"
    } else {
        "ISO-8859-1"
    }
}

/// locale。
fn locale(plan: &TargetPlan) -> Value {
    let loc = &plan.modules.locale.settings;
    let generate: Vec<Value> = loc
        .generate
        .iter()
        .map(|item| {
            let line = format!("{} {}", item.as_str(), charset(item));
            json!({ "sed": line, "line": line })
        })
        .collect();
    json!({
        "lang": loc.lang.as_str(),
        "lc_all": loc.lc_all.as_ref().map(Locale::as_str),
        "language": loc.language.as_str(),
        "generate": generate,
    })
}

/// sudoers。
fn sudoers(plan: &TargetPlan) -> Value {
    let sudo = &plan.modules.sudoers.settings;
    json!({
        "backup": sudo.backup_original.is_on(),
        "content": sudoers_content(sudo),
        "dropin": sudo.dropin_name.as_str(),
        "logfile": sudo.defaults.logfile.as_str(),
        "period": sudo.logrotate.period.as_str(),
        "rotate": sudo.logrotate.rotate.get(),
        "compress": sudo.logrotate.compress.is_on(),
    })
}

/// 服务。
fn services(plan: &TargetPlan) -> Value {
    let svc = &plan.modules.services.settings;
    let disable: Vec<&str> = svc
        .disable_now
        .iter()
        .map(xpxe_domain::init::UnitName::as_str)
        .collect();
    json!({
        "disable_now": (!disable.is_empty()).then(|| disable.join(" ")),
        "enable_now": svc.enable_now,
        "mask": svc.mask,
        "editor": svc.default_editor.as_ref().map(PlainPath::as_str),
    })
}

/// MOTD。
///
/// # Errors
///
/// 头部模板失败时返回错误。
fn motd(plan: &TargetPlan) -> Result<Value, RenderError> {
    let motd = &plan.modules.motd.settings;
    let head = &motd.custom_header;
    let header = if head.enabled.is_on() {
        let exclude_parts: Vec<String> = head
            .exclude_fs
            .iter()
            .map(|fs| format!(" -x {fs}"))
            .collect();
        let exclude = exclude_parts.concat();
        let text = render_template(
            "motd-header",
            MOTD_HEADER,
            &json!({
                "exclude_args": exclude,
                "bar_width": head.bar_width.get(),
                "warn_percent": head.warn_percent.get(),
            }),
        )?;
        Some(text.trim_end_matches('\n').to_owned())
    } else {
        None
    };
    Ok(json!({
        "clear_issue": motd.clear_issue.is_on(),
        "clear_motd": motd.empty_motd_file.is_on(),
        "disable_scripts": motd.disable_update_motd_scripts.is_on(),
        "header": header,
    }))
}

/// limits 取值文本。
fn limit_text(value: LimitValue) -> String {
    match value {
        LimitValue::Unlimited => "unlimited".to_owned(),
        LimitValue::Count(count) => count.to_string(),
    }
}

/// limits。
fn limits(plan: &TargetPlan) -> Value {
    let lim = &plan.modules.limits.settings;
    json!({
        "file_name": lim.file_name.as_str(),
        "nofile": lim.nofile.get(),
        "nproc": limit_text(lim.nproc),
        "core": limit_text(lim.core),
    })
}

/// 自定义片段：正文 base64 后由目标机解码执行，脚本里不出现原文。
fn snippets(plan: &TargetPlan) -> Value {
    let items: Vec<Value> = plan
        .modules
        .custom_snippet
        .settings
        .snippets
        .iter()
        .map(|item| {
            let body = item.body.as_str();
            let comment: String = item
                .name
                .as_str()
                .chars()
                .map(|ch| {
                    if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect();
            json!({
                "name": item.name.as_str(),
                "name_comment": comment,
                "sha256": sha256_hex(body.as_bytes()),
                "b64": STANDARD.encode(body.as_bytes()),
            })
        })
        .collect();
    json!({ "snippets": items })
}

/// 清理。
fn cleanup(plan: &TargetPlan, log_path: &PlainPath) -> Value {
    let clean = &plan.modules.cleanup.settings;
    json!({
        "log_path": log_path.as_str(),
        "trust_files": clean.remove_trust_files.is_on(),
        "apt_clean": clean.apt_clean.is_on(),
        "apt_logs": clean.delete_apt_logs.is_on(),
        "rotated_logs": clean.delete_rotated_logs.is_on(),
        "truncate_logs": clean.truncate_logs.is_on(),
        "clear_tmp": clean.clear_tmp.is_on(),
    })
}

/// APT 源。
fn apt_sources(plan: &TargetPlan, profile: &Profile) -> Value {
    let apt = &plan.modules.apt_sources.settings;
    json!({
        "disable_cdrom": apt.disable_cdrom.is_on(),
        "uncomment_http": apt.uncomment_http.is_on(),
        "fallback": apt_fallback(&apt.fallback, profile),
        "autoclean": apt.autoclean_before_update.is_on(),
    })
}

/// sshd。
fn sshd(plan: &TargetPlan) -> Value {
    let sshd = &plan.modules.sshd.settings;
    json!({
        "dropin": sshd.dropin_name.as_str(),
        "lines": sshd_lines(sshd),
        "sync_main": sshd.sync_main_config.is_on(),
        "x11": x11_value(sshd),
        "banner": sshd.banner.as_ref().map(PlainPath::as_str),
    })
}

/// shell profile。
fn shell_profile(plan: &TargetPlan) -> Value {
    let prof = &plan.modules.shell_profile.settings;
    let agent = prof
        .ssh_agent
        .enabled
        .is_on()
        .then(|| prof.ssh_agent.key.as_str());
    json!({ "aliases": aliases_content(prof), "ssh_agent": agent })
}

/// sysctl。
fn sysctl(plan: &TargetPlan) -> Value {
    let sys = &plan.modules.sysctl.settings;
    json!({
        "file_name": sys.file_name.as_str(),
        "content": sysctl_content(&sys.params, &sys.extra),
        "apply_now": sys.apply_now.is_on(),
    })
}

/// 命令历史审计。
fn history(plan: &TargetPlan) -> Value {
    let hist = &plan.modules.history_audit.settings;
    json!({
        "dir": hist.dir.as_str(),
        "mode": hist.dir_mode.as_str(),
        "histsize": hist.histsize.get(),
        "append_only": hist.append_only.is_on(),
        "source_ip": hist.timeformat_with_source_ip.is_on(),
    })
}
