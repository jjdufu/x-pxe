//! 初始化模块：默认值、范围、预设、分组与跨模块校验。

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{
    EnvName, FileMode, ModuleId, ModuleSet, PackageCatalog, PackageGroup, PlainPath, RelPath,
    SafeFileName, ShellText, SysctlKey, SysctlParams, SysctlPreset, SysctlValue, TargetPlan,
    UnitName,
};
use crate::value::SafeText;

/// 在默认计划上用 JSON Pointer 风格覆盖一个字段，返回解析结果。
///
/// # Errors
///
/// 默认计划无法序列化时返回错误。
fn with_field(path: &[&str], value: Value) -> TestResult<Result<TargetPlan, serde_json::Error>> {
    let mut doc = serde_json::to_value(TargetPlan::load_preset("deb13-init-scripts")?)?;
    let mut cursor = &mut doc;
    for key in path {
        cursor = cursor
            .get_mut(*key)
            .ok_or_else(|| format!("路径 {key} 不存在"))?;
    }
    *cursor = value;
    Ok(serde_json::from_value(doc))
}

/// 错误码集合。
fn codes(plan: &TargetPlan) -> Vec<String> {
    plan.check_with(None)
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|error| error.code)
        .collect()
}

/// 默认值全部合法：序列化后重新解析得到相同对象。
///
/// # Errors
///
/// 往返不一致时返回错误。
#[test]
fn defaults_round_trip() -> TestResult {
    let plan = TargetPlan {
        name: SafeText::parse("default")?,
        modules: ModuleSet::default(),
        finalize: super::Finalize::default(),
    };
    let json = serde_json::to_string(&plan)?;
    let back: TargetPlan = serde_json::from_str(&json)?;
    ensure_eq(&back, &plan, "默认计划往返")?;
    let minimal: TargetPlan = serde_json::from_value(json!({ "name": "x" }))?;
    ensure_eq(&minimal.modules, &plan.modules, "缺省字段取默认")?;
    ensure(
        !plan.modules.custom_snippet.enabled.is_on(),
        "自定义片段默认关闭",
    )?;
    for preset in [
        SysctlPreset::Deb13InitScripts,
        SysctlPreset::WebServer,
        SysctlPreset::Minimal,
    ] {
        let params = SysctlParams::for_preset(preset);
        let parsed: SysctlParams = serde_json::from_value(serde_json::to_value(params)?)?;
        ensure_eq(&parsed, &params, "sysctl 预设合法")?;
    }
    Ok(())
}

/// 两个预设可加载；`minimal` 只启用规定的 7 个模块与 `base_ops` 分组。
///
/// # Errors
///
/// 预设不符时返回错误。
#[test]
fn presets_load() -> TestResult {
    let full = TargetPlan::load_preset("deb13-init-scripts")?;
    for id in ModuleId::ALL {
        let expect = id != ModuleId::CustomSnippet;
        ensure_eq(&full.modules.view(id).enabled.is_on(), &expect, id.as_str())?;
    }
    let minimal = TargetPlan::load_preset("minimal")?;
    let enabled: Vec<&str> = ModuleId::ALL
        .into_iter()
        .filter(|id| minimal.modules.view(*id).enabled.is_on())
        .map(ModuleId::as_str)
        .collect();
    ensure_eq(
        &enabled,
        &vec![
            "apt_sources",
            "packages",
            "users_keys",
            "timezone",
            "time_sync",
            "locale",
            "sshd",
        ],
        "minimal 启用模块",
    )?;
    let catalog = PackageCatalog::builtin()?;
    let pkgs = minimal.modules.packages.settings.resolve(&catalog)?;
    ensure_eq(
        &pkgs.len(),
        &catalog.group(PackageGroup::BaseOps).len(),
        "minimal 只装 base_ops",
    )?;
    ensure(TargetPlan::load_preset("nope").is_err(), "未知预设")?;
    Ok(())
}

/// `ModuleSet` 的字段名与顺序等于 `ModuleId::ALL`。
///
/// # Errors
///
/// 字段名或顺序不一致时返回错误。
#[test]
fn module_ids_match_fields() -> TestResult {
    let value = serde_json::to_value(ModuleSet::default())?;
    let keys: Vec<String> = value
        .as_object()
        .ok_or("不是对象")?
        .keys()
        .cloned()
        .collect();
    let mut ids: Vec<String> = ModuleId::ALL
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    let unique: BTreeSet<&String> = ids.iter().collect();
    ensure_eq(&unique.len(), &20, "20 个唯一 id")?;
    let mut sorted_keys = keys;
    sorted_keys.sort();
    ids.sort();
    ensure_eq(&sorted_keys, &ids, "字段名 = id")?;
    Ok(())
}

/// 8 个分组的并集正好等于 legacy `PKGS`（从 `legacy/init-d13.sh` 解析）。
///
/// # Errors
///
/// legacy 无法读取或集合不等时返回错误。
#[test]
fn groups_equal_legacy_pkgs() -> TestResult {
    let legacy = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../legacy/init-d13.sh");
    let text = std::fs::read_to_string(legacy)?;
    let (_, after) = text.split_once("PKGS=(").ok_or("legacy 中没有 PKGS=(")?;
    let (body, _) = after.split_once(')').ok_or("PKGS 未闭合")?;
    let legacy_set: BTreeSet<&str> = body.split_whitespace().collect();
    let catalog = PackageCatalog::builtin()?;
    let all = catalog.all_packages();
    let ours: BTreeSet<&str> = all.iter().map(crate::value::PackageName::as_str).collect();
    ensure_eq(&ours.len(), &80, "80 个包")?;
    ensure_eq(&all.len(), &80, "分组之间不重复")?;
    ensure_eq(&ours, &legacy_set, "分组并集 = legacy PKGS")?;
    Ok(())
}

/// 范围用例：字段路径（`/` 分隔）、下界、上界。
const RANGE_CASES: [(&str, u64, u64); 6] = [
    (
        "modules/systemd_manager/settings/default_timeout_start_s",
        5,
        600,
    ),
    ("modules/sudoers/settings/defaults/passwd_tries", 1, 10),
    ("modules/motd/settings/custom_header/bar_width", 20, 120),
    ("modules/limits/settings/nofile", 1024, 1_048_576),
    ("modules/history_audit/settings/histsize", 1000, 1_000_000),
    ("modules/sysctl/settings/params/memory/swappiness", 0, 100),
];

/// 检查一个范围字段的四个边界。
///
/// # Errors
///
/// 边界判断不符时返回错误。
fn check_range(label: &str, low: u64, high: u64) -> TestResult {
    let path: Vec<&str> = label.split('/').collect();
    ensure(
        with_field(&path, json!(low))?.is_ok(),
        &format!("{label} 下界"),
    )?;
    ensure(
        with_field(&path, json!(high))?.is_ok(),
        &format!("{label} 上界"),
    )?;
    ensure(
        with_field(&path, json!(high.saturating_add(1)))?.is_err(),
        &format!("{label} 超上界"),
    )?;
    if let Some(below) = low.checked_sub(1) {
        ensure(
            with_field(&path, json!(below))?.is_err(),
            &format!("{label} 低于下界"),
        )?;
    }
    Ok(())
}

/// 范围边界：界内接受，界外拒绝；未知字段拒绝。
///
/// # Errors
///
/// 边界判断不符时返回错误。
#[test]
fn ranges_enforced() -> TestResult {
    for (label, low, high) in RANGE_CASES {
        check_range(label, low, high)?;
    }
    let unknown = with_field(&["modules", "sshd", "settings"], json!({ "nope": true }))?;
    ensure(unknown.is_err(), "未知字段被拒绝")?;
    Ok(())
}

/// 白名单类型拒绝 shell 元字符与路径穿越。
///
/// # Errors
///
/// 非法输入被接受时返回错误。
#[test]
fn types_reject_metacharacters() -> TestResult {
    let evil = [
        "a;b", "a b", "$(id)", "`id`", "a'b", "a\"b", "a\nb", "../x", "a|b", "",
    ];
    for raw in evil {
        ensure(UnitName::parse(raw).is_err(), &format!("UnitName {raw:?}"))?;
        ensure(
            SafeFileName::parse(raw).is_err(),
            &format!("SafeFileName {raw:?}"),
        )?;
        ensure(RelPath::parse(raw).is_err(), &format!("RelPath {raw:?}"))?;
        ensure(
            SysctlKey::parse(raw).is_err(),
            &format!("SysctlKey {raw:?}"),
        )?;
        ensure(EnvName::parse(raw).is_err(), &format!("EnvName {raw:?}"))?;
        ensure(
            PlainPath::parse(&format!("/{raw}")).is_err() || raw.is_empty(),
            &format!("PlainPath {raw:?}"),
        )?;
    }
    ensure(
        PlainPath::parse("/usr/bin/../sbin").is_err(),
        "PlainPath 点段",
    )?;
    ensure(
        PlainPath::parse("/usr/bin/vim.basic").is_ok(),
        "PlainPath 合法",
    )?;
    ensure(
        SysctlValue::parse("4096 87380 12582912").is_ok(),
        "SysctlValue 三元组",
    )?;
    ensure(
        SysctlValue::parse("1; reboot").is_ok(),
        "分号在 sysctl 文件中无害",
    )?;
    ensure(SysctlValue::parse("$(id)").is_err(), "SysctlValue 拒绝 $")?;
    ensure(
        FileMode::parse("1777").is_ok() && FileMode::parse("8777").is_err(),
        "FileMode",
    )?;
    ensure(ShellText::parse("a\0b").is_err(), "ShellText NUL")?;
    ensure(
        ShellText::parse(&"x".repeat(ShellText::MAX_LEN + 1)).is_err(),
        "ShellText 64 KiB",
    )?;
    Ok(())
}

/// 禁 root 且禁口令时必须有可登录的 sudo 用户。
///
/// # Errors
///
/// 校验结果不符时返回错误。
#[test]
fn login_path_required() -> TestResult {
    let mut plan = TargetPlan::load_preset("deb13-init-scripts")?;
    ensure(codes(&plan).is_empty(), "deb13-init-scripts 通过")?;
    plan.modules
        .users_keys
        .settings
        .root_authorized_keys
        .clear();
    for user in &mut plan.modules.users_keys.settings.users {
        user.authorized_keys.clear();
    }
    ensure_eq(&codes(&plan), &vec!["no_login_path".to_owned()], "没有公钥")?;
    plan.modules.sshd.settings.password_authentication = super::Flag::ON;
    ensure(codes(&plan).is_empty(), "开启口令认证后放行")?;
    Ok(())
}

/// 引用未知用户、重名、端口与缓冲顺序、LANGUAGE 字符集。
///
/// # Errors
///
/// 校验结果不符时返回错误。
#[test]
fn cross_module_rules() -> TestResult {
    let mut plan = TargetPlan::load_preset("deb13-init-scripts")?;
    plan.modules
        .vimrc
        .settings
        .targets
        .push(crate::value::UnixUser::parse("ghost")?);
    plan.modules
        .sudoers
        .settings
        .full_sudo_users
        .push(crate::value::UnixUser::parse("ghost")?);
    let dup = plan
        .modules
        .users_keys
        .settings
        .users
        .first()
        .cloned()
        .ok_or("缺用户")?;
    plan.modules.users_keys.settings.users.push(dup);
    let params = &mut plan.modules.sysctl.settings.params;
    params.memory.dirty_background_ratio = super::Ranged::new(20)?;
    params.conn_table.ip_local_port_range.low = super::Ranged::new(65_000)?;
    params.conn_table.ip_local_port_range.high = super::Ranged::new(2000)?;
    params.buffers.tcp_rmem.default = super::Ranged::new(1 << 29)?;
    plan.modules.locale.settings.language = SafeText::parse("en\"; id")?;
    let mut got = codes(&plan);
    got.sort();
    let mut want: Vec<String> = [
        "unknown_user",
        "unknown_user",
        "duplicate_user",
        "out_of_order",
        "out_of_order",
        "out_of_order",
        "bad_language",
    ]
    .iter()
    .map(|code| (*code).to_owned())
    .collect();
    want.sort();
    ensure_eq(&got, &want, "全部违反项")?;
    let primary = crate::provision::PrimaryUser {
        name: crate::value::UnixUser::parse("ghost")?,
        full_name: SafeText::parse("Ghost")?,
        hash: crate::provision::CryptSecret::parse(
            "$6$xpxesaltxpxesalt$iFdzdNjxB/OBPF2XSlT.u.sRRMJraJo0vQhUZ5SC65wTzKtg3D9EK5pe8kURU4jdOBRVVOFFTXkn0lEjKv7Am.",
        )?,
        groups: Vec::new(),
    };
    let with_primary = plan.check_with(Some(&primary)).err().unwrap_or_default();
    ensure(
        with_primary
            .iter()
            .all(|error| error.code != "unknown_user"),
        "主用户可被引用",
    )?;
    Ok(())
}

/// 警告：nftables 被 mask 且安装 fail2ban；禁用 IPv6。
///
/// # Errors
///
/// 警告不符时返回错误。
#[test]
fn warnings_reported() -> TestResult {
    let plan = TargetPlan::load_preset("deb13-init-scripts")?;
    let codes: Vec<String> = plan
        .warnings()
        .into_iter()
        .map(|error| error.code)
        .collect();
    ensure_eq(
        &codes,
        &vec![
            "fail2ban_without_firewall".to_owned(),
            "confirm_no_dual_stack".to_owned(),
        ],
        "deb13-init-scripts 警告",
    )?;
    let minimal = TargetPlan::load_preset("minimal")?;
    ensure(minimal.warnings().is_empty(), "minimal 无警告")?;
    Ok(())
}

/// extra/exclude/zram 的解析，以及 extra 与 exclude 冲突。
///
/// # Errors
///
/// 清单不符时返回错误。
#[test]
fn package_resolution() -> TestResult {
    let catalog = PackageCatalog::builtin()?;
    let mut plan = TargetPlan::load_preset("deb13-init-scripts")?;
    let pkgs = &mut plan.modules.packages.settings;
    pkgs.exclude
        .push(crate::value::PackageName::parse("clamav")?);
    pkgs.extra.push(crate::value::PackageName::parse("htop")?);
    pkgs.zram_tools = super::Flag::ON;
    let list = pkgs.resolve(&catalog)?;
    let names: Vec<&str> = list.iter().map(crate::value::PackageName::as_str).collect();
    ensure(!names.contains(&"clamav"), "排除生效")?;
    ensure(
        names.contains(&"htop") && names.contains(&"zram-tools"),
        "额外包",
    )?;
    ensure_eq(&names.len(), &81, "80 - 1 + 2")?;
    pkgs.extra.push(crate::value::PackageName::parse("clamav")?);
    ensure_eq(&codes(&plan), &vec!["extra_excluded".to_owned()], "冲突")?;
    Ok(())
}
