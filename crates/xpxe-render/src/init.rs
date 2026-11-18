//! 初始化计划渲染：每个模块一份 `#!/bin/bash` + `set -euo pipefail` 的独立脚本，
//! 汇总为目标机 `xpxe-agent` 执行的计划文件。

mod context;
mod diff;
mod files;
mod snippet;

pub use diff::{DiffLine, diff_lines};

use minijinja::syntax::SyntaxConfig;
use minijinja::value::Serde;
use minijinja::{Environment, Value};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use xpxe_domain::init::{
    ModuleId, PLAN_FILE_SCHEMA, PlainPath, PlanEntry, PlanFile, PlanFinalize, TargetPlan,
};
use xpxe_domain::versioning::field_error;
use xpxe_domain::{FieldError, HttpUrl, Profile, SessionId};

use crate::RenderError;
use crate::filters::sh_quote;

/// 默认执行日志路径。
pub const DEFAULT_LOG_PATH: &str = "/var/log/xpxe-firstboot.log";

/// 渲染一份计划所需的会话信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanContext {
    /// 装机会话。
    pub session: SessionId,
    /// 事件回报地址。
    pub events_url: HttpUrl,
    /// 执行日志路径。
    pub log_path: PlainPath,
    /// 最终事件持续重试时长（秒，等于 `configure_ttl`）。
    pub final_retry_s: u64,
}

/// 渲染好的一个模块。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderedModule {
    /// 模块。
    pub id: ModuleId,
    /// 是否执行。
    pub enabled: bool,
    /// 脚本正文（禁用模块也渲染，供预览；计划文件中为空串）。
    pub script: String,
}

/// 渲染好的计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPlan {
    /// 逐模块脚本。
    pub modules: Vec<RenderedModule>,
    /// 计划文件对象。
    pub file: PlanFile,
    /// 计划文件 JSON（下发到 `/var/lib/xpxe/plan.json`）。
    pub json: String,
    /// 不阻止渲染的警告。
    pub warnings: Vec<FieldError>,
}

/// 一个模块两版脚本的差异。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleDiff {
    /// 模块。
    pub id: ModuleId,
    /// 逐行差异。
    pub lines: Vec<DiffLine>,
}

/// 计算 SHA-256 十六进制。
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// 校验计划（含 Profile 主用户与自定义片段语法）并返回警告。
///
/// # Errors
///
/// 跨模块校验或片段结构检查失败时返回错误。
pub fn check_plan(plan: &TargetPlan, profile: &Profile) -> Result<Vec<FieldError>, RenderError> {
    let primary = profile.accounts().primary_user.as_ref();
    plan.check_with(primary).map_err(|errors| {
        let text: Vec<String> = errors.iter().map(ToString::to_string).collect();
        RenderError::new("invalid_plan", text.join("; "))
    })?;
    let mut warnings = plan.warnings();
    for (index, item) in plan
        .modules
        .custom_snippet
        .settings
        .snippets
        .iter()
        .enumerate()
    {
        let path = format!("/modules/custom_snippet/settings/snippets/{index}/body");
        let found = snippet::check_shell(item.body.as_str())
            .map_err(|error| RenderError::new("bad_snippet", format!("{path}: {error}")))?;
        warnings.extend(
            found
                .iter()
                .map(|message| field_error(&path, "bashism", message)),
        );
    }
    Ok(warnings)
}

/// 渲染单个模块脚本（Web 的“模块预览”）。
///
/// # Errors
///
/// 模板失败时返回错误。
pub fn render_module(
    plan: &TargetPlan,
    profile: &Profile,
    id: ModuleId,
    log_path: &PlainPath,
) -> Result<String, RenderError> {
    let (name, source) = template_for(id);
    let ctx = context::build(id, plan, profile, log_path)?;
    let body = render_template(name, source, &ctx)?;
    Ok(format!(
        "#!/bin/bash\n# x-pxe 初始化模块 {}：{}\nset -euo pipefail\n\n{}\n",
        id.as_str(),
        id.title(),
        body.trim_end_matches('\n')
    ))
}

/// 渲染整份计划。
///
/// # Errors
///
/// 校验、模板或 JSON 序列化失败时返回错误。
pub fn render_plan(
    plan: &TargetPlan,
    profile: &Profile,
    ctx: &PlanContext,
) -> Result<RenderedPlan, RenderError> {
    let warnings = check_plan(plan, profile)?;
    let mut modules = Vec::new();
    let mut entries = Vec::new();
    for id in ModuleId::ALL {
        let view = plan.modules.view(id);
        let script = render_module(plan, profile, id, &ctx.log_path)?;
        let shipped = if view.enabled.is_on() {
            script.clone()
        } else {
            String::new()
        };
        entries.push(PlanEntry {
            id: id.as_str().to_owned(),
            title: id.title().to_owned(),
            enabled: view.enabled.is_on(),
            timeout_s: view.timeout_s,
            continue_on_error: view.continue_on_error.is_on(),
            sha256: sha256_hex(shipped.as_bytes()),
            script: shipped,
        });
        modules.push(RenderedModule {
            id,
            enabled: view.enabled.is_on(),
            script,
        });
    }
    let file = PlanFile {
        schema: PLAN_FILE_SCHEMA,
        session: ctx.session.as_str().to_owned(),
        events_url: ctx.events_url.as_str().to_owned(),
        log_path: ctx.log_path.as_str().to_owned(),
        final_retry_s: ctx.final_retry_s,
        modules: entries,
        finalize: PlanFinalize {
            reboot: plan.finalize.reboot.is_on(),
        },
    };
    let json = serde_json::to_string_pretty(&file)
        .map_err(|error| RenderError::new("plan_json", error.to_string()))?;
    Ok(RenderedPlan {
        modules,
        file,
        json,
        warnings,
    })
}

/// 两个版本逐模块脚本差异（只列出有变化的模块）。
#[must_use]
pub fn diff_plans(old: &RenderedPlan, new: &RenderedPlan) -> Vec<ModuleDiff> {
    old.modules
        .iter()
        .zip(new.modules.iter())
        .filter(|(left, right)| left.script != right.script || left.enabled != right.enabled)
        .map(|(left, right)| ModuleDiff {
            id: right.id,
            lines: diff_lines(&left.script, &right.script),
        })
        .collect()
}

/// 模块模板。
const fn template_for(id: ModuleId) -> (&'static str, &'static str) {
    match id {
        ModuleId::AptSources => (
            "apt_sources",
            include_str!("templates/init/apt_sources.sh.j2"),
        ),
        ModuleId::Packages => ("packages", include_str!("templates/init/packages.sh.j2")),
        ModuleId::UsersKeys => (
            "users_keys",
            include_str!("templates/init/users_keys.sh.j2"),
        ),
        ModuleId::Timezone => ("timezone", include_str!("templates/init/timezone.sh.j2")),
        ModuleId::TimeSync => ("time_sync", include_str!("templates/init/time_sync.sh.j2")),
        ModuleId::Locale => ("locale", include_str!("templates/init/locale.sh.j2")),
        ModuleId::SystemdManager => (
            "systemd_manager",
            include_str!("templates/init/systemd_manager.sh.j2"),
        ),
        ModuleId::SshGeneratorFix => (
            "ssh_generator_fix",
            include_str!("templates/init/ssh_generator_fix.sh.j2"),
        ),
        ModuleId::Sshd => ("sshd", include_str!("templates/init/sshd.sh.j2")),
        ModuleId::Sudoers => ("sudoers", include_str!("templates/init/sudoers.sh.j2")),
        ModuleId::ShellProfile => (
            "shell_profile",
            include_str!("templates/init/shell_profile.sh.j2"),
        ),
        ModuleId::Vimrc => ("vimrc", include_str!("templates/init/vimrc.sh.j2")),
        ModuleId::Services => ("services", include_str!("templates/init/services.sh.j2")),
        ModuleId::Motd => ("motd", include_str!("templates/init/motd.sh.j2")),
        ModuleId::Sysctl => ("sysctl", include_str!("templates/init/sysctl.sh.j2")),
        ModuleId::Limits => ("limits", include_str!("templates/init/limits.sh.j2")),
        ModuleId::HistoryAudit => (
            "history_audit",
            include_str!("templates/init/history_audit.sh.j2"),
        ),
        ModuleId::Fstrim => ("fstrim", include_str!("templates/init/fstrim.sh.j2")),
        ModuleId::CustomSnippet => (
            "custom_snippet",
            include_str!("templates/init/custom_snippet.sh.j2"),
        ),
        ModuleId::Cleanup => ("cleanup", include_str!("templates/init/cleanup.sh.j2")),
    }
}

/// 用 `trim_blocks` / `lstrip_blocks` 渲染模板。
///
/// # Errors
///
/// 模板语法或渲染失败时返回错误。
pub(crate) fn render_template<C: Serialize>(
    name: &str,
    source: &str,
    ctx: &C,
) -> Result<String, RenderError> {
    let syntax = SyntaxConfig::builder()
        .trim_blocks(true)
        .lstrip_blocks(true)
        .keep_trailing_newline(true)
        .build()
        .map_err(|error| RenderError::new("template_syntax", error.to_string()))?;
    let mut env = Environment::new();
    env.set_syntax(syntax);
    env.add_filter("sh_quote", |value: &Value| sh_quote(&value.to_string()));
    env.add_template(name, source)
        .map_err(|error| RenderError::new("template_parse", format!("{name}: {error}")))?;
    let template = env
        .get_template(name)
        .map_err(|error| RenderError::new("template_missing", format!("{name}: {error}")))?;
    template
        .render(Serde(ctx))
        .map_err(|error| RenderError::new("template_render", format!("{name}: {error}")))
}

#[cfg(test)]
mod tests;
