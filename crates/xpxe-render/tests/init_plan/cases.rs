//! 初始化计划渲染用例。

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use xpxe_domain::init::{Flag, ModuleId, PlainPath, Ranged, ShellText, Snippet, TargetPlan};
use xpxe_domain::{HttpUrl, Profile, SafeText, SessionId, SshPublicKey};
use xpxe_render::init::{
    DEFAULT_LOG_PATH, DiffLine, PlanContext, RenderedPlan, diff_plans, render_plan, sha256_hex,
};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

/// 渲染上下文。
///
/// # Errors
///
/// 解析失败时返回错误。
fn ctx() -> TestResult<PlanContext> {
    Ok(PlanContext {
        session: SessionId::parse("sess01")?,
        events_url: HttpUrl::parse("http://10.77.0.1:8080/p/agent001/events")?,
        log_path: PlainPath::parse(DEFAULT_LOG_PATH)?,
        final_retry_s: 600,
    })
}

/// 渲染某预设。
///
/// # Errors
///
/// 加载或渲染失败时返回错误。
fn render(plan: &TargetPlan) -> TestResult<RenderedPlan> {
    let profile = Profile::load_preset("official-mirror")?;
    Ok(render_plan(plan, &profile, &ctx()?)?)
}

/// 黄金目录。
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/init_plan/golden")
}

/// 与黄金文件比较；设置 `XPXE_UPDATE_GOLDEN=1` 时覆盖。
///
/// # Errors
///
/// 内容不一致或文件缺失时返回错误。
fn compare_golden(name: &str, actual: &str) -> TestResult {
    let path = golden_dir().join(name);
    if std::env::var("XPXE_UPDATE_GOLDEN").is_ok() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, actual)?;
        return Ok(());
    }
    let expected = fs::read_to_string(&path)
        .map_err(|error| format!("缺少黄金文件 {name}（{error}），可设 XPXE_UPDATE_GOLDEN=1"))?;
    ensure_eq(actual, expected.as_str(), name)?;
    Ok(())
}

/// 用 `bash -n` 检查语法。
///
/// # Errors
///
/// 语法错误或无法启动 bash 时返回错误。
fn bash_syntax(label: &str, script: &str) -> TestResult {
    let dir = std::env::temp_dir().join(format!("xpxe-init-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{label}.sh"));
    fs::write(&file, script)?;
    let out = Command::new("bash").arg("-n").arg(&file).output()?;
    ensure(
        out.status.success(),
        &format!(
            "{label}: bash -n 失败：{}",
            String::from_utf8_lossy(&out.stderr)
        ),
    )?;
    Ok(())
}

/// 两个预设的逐模块快照与 `bash -n`。
///
/// # Errors
///
/// 快照不符或语法错误时返回错误。
#[test]
fn presets_snapshot_and_syntax() -> TestResult {
    for preset in TargetPlan::PRESETS {
        let rendered = render(&TargetPlan::load_preset(preset)?)?;
        ensure_eq(&rendered.modules.len(), &20, "20 个模块")?;
        for module in &rendered.modules {
            let label = format!("{preset}-{}", module.id.as_str());
            ensure(module.script.starts_with("#!/bin/bash\n"), &label)?;
            ensure(module.script.contains("\nset -euo pipefail\n"), &label)?;
            bash_syntax(&label, &module.script)?;
            compare_golden(
                &format!("{preset}/{}.sh", module.id.as_str()),
                &module.script,
            )?;
        }
        compare_golden(&format!("{preset}/plan.json"), &rendered.json)?;
    }
    Ok(())
}

/// 计划文件：顺序、哈希、禁用模块为空脚本、收尾与会话字段。
///
/// # Errors
///
/// 字段不符时返回错误。
#[test]
fn plan_file_shape() -> TestResult {
    let rendered = render(&TargetPlan::load_preset("minimal")?)?;
    let file = &rendered.file;
    let ids: Vec<&str> = file.modules.iter().map(|entry| entry.id.as_str()).collect();
    let want: Vec<&str> = ModuleId::ALL.iter().map(|id| id.as_str()).collect();
    ensure_eq(&ids, &want, "顺序")?;
    for entry in &file.modules {
        ensure_eq(
            &entry.sha256,
            &sha256_hex(entry.script.as_bytes()),
            &entry.id,
        )?;
        ensure(
            entry.enabled != entry.script.is_empty(),
            &format!("{} 启用↔有脚本", entry.id),
        )?;
    }
    ensure_eq(
        &file.modules.iter().filter(|entry| entry.enabled).count(),
        &7,
        "minimal 7 个启用",
    )?;
    ensure(file.finalize.reboot, "reboot")?;
    ensure_eq(&file.session.as_str(), &"sess01", "会话")?;
    ensure_eq(&file.final_retry_s, &600, "最终重试")?;
    let parsed: xpxe_domain::init::PlanFile = serde_json::from_str(&rendered.json)?;
    ensure_eq(&&parsed, &file, "JSON 往返")?;
    let packages = file.modules.get(1).ok_or("缺 packages")?;
    ensure_eq(&packages.timeout_s, &3600, "packages 默认超时")?;
    Ok(())
}

/// 取出脚本中某个 here-doc 的正文。
fn heredoc<'a>(script: &'a str, opener: &str, end: &str) -> Option<&'a str> {
    let start = script.find(opener)?;
    let body_start = script
        .get(start..)?
        .find('\n')?
        .checked_add(start)?
        .checked_add(1)?;
    let rest = script.get(body_start..)?;
    let stop = rest.find(&format!("\n{end}\n"))?;
    rest.get(..=stop)
}

/// 取出 legacy 中 `cat > <path> << 'EOF'` 的正文。
fn legacy_block<'a>(legacy: &'a str, path: &str) -> Option<&'a str> {
    heredoc(legacy, &format!("cat > {path} << 'EOF'"), "EOF")
}

/// legacy 文件对照表：模块、渲染脚本中的 here-doc 开头、legacy 中的目标路径。
const LEGACY_FILES: [(ModuleId, &str, &str); 13] = [
    (
        ModuleId::Sshd,
        "cat > /etc/ssh/sshd_config.d/jwczjzl.conf << 'XPXE_EOF'",
        "/etc/ssh/sshd_config.d/jwczjzl.conf",
    ),
    (
        ModuleId::Sysctl,
        "cat > /etc/sysctl.d/99-fleet-netvm.conf << 'XPXE_EOF'",
        "/etc/sysctl.d/99-fleet-netvm.conf",
    ),
    (
        ModuleId::Limits,
        "cat > /etc/security/limits.d/90-nofile.conf << 'XPXE_EOF'",
        "/etc/security/limits.d/90-nofile.conf",
    ),
    (
        ModuleId::Sudoers,
        "cat > \"${tmp}\" << 'XPXE_EOF'",
        "/etc/sudoers.d/jwczjzl",
    ),
    (
        ModuleId::Sudoers,
        "cat > /etc/logrotate.d/sudo << 'XPXE_EOF'",
        "/etc/logrotate.d/sudo",
    ),
    (
        ModuleId::ShellProfile,
        "cat > /etc/profile.d/zz-aliases.sh << 'XPXE_EOF'",
        "/etc/profile.d/zz-aliases.sh",
    ),
    (
        ModuleId::ShellProfile,
        "cat > /etc/profile.d/ssh-agent.sh << 'XPXE_EOF'",
        "/etc/profile.d/ssh-agent.sh",
    ),
    (
        ModuleId::HistoryAudit,
        "cat > /etc/profile.d/history-record.sh << 'XPXE_EOF'",
        "/etc/profile.d/history-record.sh",
    ),
    (
        ModuleId::Vimrc,
        "cat > \"${home}/.vimrc\" << 'XPXE_VIMRC'",
        "/root/.vimrc",
    ),
    (
        ModuleId::Locale,
        "cat > /etc/default/locale << 'XPXE_EOF'",
        "/etc/default/locale",
    ),
    (
        ModuleId::SystemdManager,
        "cat > /etc/systemd/system.conf.d/timeout.conf << 'XPXE_EOF'",
        "/etc/systemd/system.conf.d/timeout.conf",
    ),
    (
        ModuleId::TimeSync,
        "cat > /etc/systemd/timesyncd.conf.d/ntp.conf << 'XPXE_EOF'",
        "/etc/systemd/timesyncd.conf.d/ntp.conf",
    ),
    (
        ModuleId::AptSources,
        "cat > /etc/apt/sources.list.d/debian.sources << 'XPXE_EOF'",
        "/etc/apt/sources.list.d/debian.sources",
    ),
];

/// deb13-init-scripts 渲染出的配置文件与 legacy 中的同名文件逐字节一致。
///
/// # Errors
///
/// 内容不一致时返回错误。
#[test]
fn files_match_legacy() -> TestResult {
    let legacy = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../legacy/init-d13.sh"),
    )?;
    let rendered = render(&TargetPlan::load_preset("deb13-init-scripts")?)?;
    let script = |id: ModuleId| {
        rendered
            .modules
            .iter()
            .find(|module| module.id == id)
            .map_or_else(String::new, |module| module.script.clone())
    };
    for (id, opener, path) in LEGACY_FILES {
        let text = script(id);
        let end = if opener.contains("XPXE_VIMRC") {
            "XPXE_VIMRC"
        } else {
            "XPXE_EOF"
        };
        let ours =
            heredoc(&text, opener, end).ok_or_else(|| format!("{path}: 渲染中没有 here-doc"))?;
        let theirs = legacy_block(&legacy, path).ok_or_else(|| format!("{path}: legacy 中没有"))?;
        ensure_eq(ours, theirs, path)?;
    }
    Ok(())
}

/// 字符串字段中的 shell 元字符只会以单引号转义形式出现，片段正文不以明文出现。
///
/// # Errors
///
/// 出现未转义插值时返回错误。
#[test]
fn injection_is_quoted() -> TestResult {
    let mut plan = TargetPlan::load_preset("deb13-init-scripts")?;
    let evil_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBtCscA5CqkUDclClV2dKSGH4JxR2vLulpTgLR34Pz1l x'$(id)`id`;rm -rf /";
    let key = SshPublicKey::parse(evil_key)?;
    plan.modules.users_keys.settings.root_authorized_keys = vec![key];
    plan.modules.custom_snippet.enabled = Flag::ON;
    plan.modules.custom_snippet.settings.snippets = vec![Snippet {
        name: SafeText::parse("n'$(reboot)")?,
        body: ShellText::parse("echo \"$(uname -r)\"\nif true; then echo ok; fi\n")?,
    }];
    let rendered = render(&plan)?;
    for module in &rendered.modules {
        bash_syntax(&format!("evil-{}", module.id.as_str()), &module.script)?;
    }
    let users = &rendered.modules.get(2).ok_or("缺 users_keys")?.script;
    let quoted = "'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBtCscA5CqkUDclClV2dKSGH4JxR2vLulpTgLR34Pz1l x'\\''$(id)`id`;rm -rf /'";
    ensure(users.contains(quoted), "公钥被单引号转义")?;
    ensure(!users.contains(&format!("echo {evil_key}")), "没有裸插值")?;
    let snippet = &rendered.modules.get(18).ok_or("缺 custom_snippet")?.script;
    ensure(!snippet.contains("uname -r"), "片段正文不以明文出现")?;
    ensure(snippet.contains("'n'\\''$(reboot)'"), "片段名被转义")?;
    ensure(
        !snippet.contains("# ---- 片段 1：n'"),
        "注释中的名称被替换为安全字符",
    )?;
    Ok(())
}

/// 结构错误的片段被拒绝；bash 专有语法只给警告。
///
/// # Errors
///
/// 判断不符时返回错误。
#[test]
fn snippet_checks() -> TestResult {
    let mut plan = TargetPlan::load_preset("deb13-init-scripts")?;
    plan.modules.custom_snippet.enabled = Flag::ON;
    plan.modules.custom_snippet.settings.snippets = vec![Snippet {
        name: SafeText::parse("bad")?,
        body: ShellText::parse("if true; then echo x\n")?,
    }];
    let profile = Profile::load_preset("official-mirror")?;
    ensure(
        render_plan(&plan, &profile, &ctx()?).is_err(),
        "未闭合 if 被拒绝",
    )?;
    plan.modules.custom_snippet.settings.snippets = vec![Snippet {
        name: SafeText::parse("bashism")?,
        body: ShellText::parse("if [[ -d /x ]]; then echo y; fi\n")?,
    }];
    let rendered = render_plan(&plan, &profile, &ctx()?)?;
    ensure(
        rendered
            .warnings
            .iter()
            .any(|warning| warning.code == "bashism"),
        "bash 警告",
    )?;
    Ok(())
}

/// 改一个 sysctl 字段，只有 sysctl 模块出现差异。
///
/// # Errors
///
/// 差异不符时返回错误。
#[test]
fn diff_between_versions() -> TestResult {
    let old_plan = TargetPlan::load_preset("deb13-init-scripts")?;
    let mut new_plan = old_plan.clone();
    new_plan.modules.sysctl.settings.params.memory.swappiness = Ranged::new(1)?;
    let diffs = diff_plans(&render(&old_plan)?, &render(&new_plan)?);
    ensure_eq(&diffs.len(), &1, "一个模块变化")?;
    let first = diffs.first().ok_or("无差异")?;
    ensure_eq(&first.id, &ModuleId::Sysctl, "sysctl")?;
    let changed: Vec<&DiffLine> = first
        .lines
        .iter()
        .filter(|line| !matches!(line, DiffLine::Same(_)))
        .collect();
    ensure_eq(
        &changed,
        &vec![
            &DiffLine::Removed("vm.swappiness = 10".to_owned()),
            &DiffLine::Added("vm.swappiness = 1".to_owned()),
        ],
        "一删一增",
    )?;
    Ok(())
}
