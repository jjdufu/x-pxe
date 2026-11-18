//! 选盘脚本与自定义 `expert_recipe`。

use serde::Serialize;
use xpxe_domain::{
    DiskMatch, DiskRule, FsType, MountPoint, PartitionScheme, PartitionSpec, Prefer, SizeSpec,
};

use crate::RenderError;
use crate::filters::sh_quote;
use crate::request::RenderRequest;
use crate::template::render_named;

/// 选盘脚本模板。
const DISK_SELECT: &str = include_str!("templates/disk-select.sh.j2");
/// partman early 钩子。
const PARTMAN_EARLY: &str = include_str!("templates/partman-early.sh.j2");
/// late 钩子。
const LATE: &str = include_str!("templates/late.sh.j2");
/// firstboot 单元。
const FIRSTBOOT: &str = include_str!("templates/firstboot.service.j2");

/// 与选盘相关的渲染产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskArtifacts {
    /// 选盘脚本。
    pub disk_select: String,
    /// partman early 钩子。
    pub partman_early: String,
    /// late 钩子。
    pub late: String,
    /// firstboot 单元。
    pub firstboot_unit: String,
}

/// 渲染选盘脚本、partman / late 钩子。
///
/// # Errors
///
/// 模板失败时返回错误。
pub(crate) fn render_disk_artifacts(request: &RenderRequest) -> Result<DiskArtifacts, RenderError> {
    let disk_select = render_disk_select(request)?;
    let recipes = custom_recipes(request.profile().partitioning());
    let partman_early = render_named(
        "partman-early.sh",
        PARTMAN_EARLY,
        &PartmanContext {
            disk_url: format!("{}/disk-select.sh", request.session_base()),
            fail_prefix: format!(
                "event=installer.failed event_id={} reason=disk_selection inventory=",
                request.event_id().as_str()
            ),
            fail_url: format!("{}/events/installer.failed", request.boot_base().as_str()),
            custom: recipes.is_some(),
            uefi_recipe: recipes.as_ref().map_or("", |pair| pair.uefi.as_str()),
            bios_recipe: recipes.as_ref().map_or("", |pair| pair.bios.as_str()),
        },
    )?;
    let unit = render_named("firstboot.service", FIRSTBOOT, &Empty)?;
    let late = render_late(request, &unit)?;
    Ok(DiskArtifacts {
        disk_select,
        partman_early,
        late,
        firstboot_unit: unit,
    })
}

/// 仅选盘脚本（供库入口与测试）。
///
/// # Errors
///
/// 模板失败时返回错误。
pub(crate) fn render_disk_select(request: &RenderRequest) -> Result<String, RenderError> {
    let rule = request.profile().disk();
    let max = rule
        .size_gib
        .max
        .map_or_else(String::new, |value| value.to_string());
    let exclude_block = exclude_lines(rule);
    render_named(
        "disk-select.sh",
        DISK_SELECT,
        &DiskContext {
            min_gib: rule.size_gib.min,
            max_gib: max,
            prefer: prefer_name(rule.prefer),
            exclude_block,
            include_body: include_body(rule),
        },
    )
}

/// 排除列表（heredoc 正文）。
fn exclude_lines(rule: &DiskRule) -> String {
    rule.exclude_models
        .iter()
        .map(xpxe_domain::SafeText::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

/// prefer 标识。
const fn prefer_name(prefer: Prefer) -> &'static str {
    match prefer {
        Prefer::Unique => "unique",
        Prefer::Smallest => "smallest",
        Prefer::Largest => "largest",
    }
}

/// `include_ok` 函数体。无规则时直接成功。
fn include_body(rule: &DiskRule) -> String {
    if rule.include.is_empty() {
        return "    return 0".to_owned();
    }
    let mut lines = vec!["    matched=1".to_owned()];
    for item in &rule.include {
        lines.push(include_rule(item));
    }
    lines.push("    return \"$matched\"".to_owned());
    lines.join("\n")
}

/// 单条 include 规则的 shell 片段。
fn include_rule(item: &DiskMatch) -> String {
    match item {
        DiskMatch::Transport { kind } => format!(
            "    [ \"$transport\" = {want} ] && return 0",
            want = sh_quote(kind.as_str())
        ),
        DiskMatch::Serial { value } | DiskMatch::Wwn { value } => format!(
            "    [ \"$serial\" = {want} ] && return 0",
            want = sh_quote(value.as_str())
        ),
        DiskMatch::Model { regex } => format!(
            "    echo \"$model\" | grep -E -q {pat} && return 0",
            pat = sh_quote(regex.as_str())
        ),
        DiskMatch::ById { glob } => by_id_rule(glob.as_str()),
    }
}

/// by-id 通配规则。
fn by_id_rule(glob: &str) -> String {
    format!(
        "    if [ -d \"$BYID\" ]; then\n      for link in \"$BYID\"/*; do\n        [ -L \"$link\" ] || continue\n        id=$(basename \"$link\")\n        case \"$id\" in\n          {glob})\n            target=$(readlink \"$link\" || true)\n            [ \"${{target##*/}}\" = \"$name\" ] && return 0\n            ;;\n        esac\n      done\n    fi"
    )
}

/// 自定义配方的 UEFI / BIOS 文本。
struct RecipePair {
    /// UEFI（含 512 MiB ESP）。
    uefi: String,
    /// BIOS（含 1 MiB `biosgrub`）。
    bios: String,
}

/// 生成自定义 `expert_recipe`；Atomic 方案返回 `None`。
fn custom_recipes(scheme: &PartitionScheme) -> Option<RecipePair> {
    match scheme {
        PartitionScheme::AtomicLvm | PartitionScheme::AtomicPlain => None,
        PartitionScheme::Custom { partitions } => Some(RecipePair {
            uefi: build_recipe(partitions, &RecipeFirmware::Uefi),
            bios: build_recipe(partitions, &RecipeFirmware::Bios),
        }),
    }
}

/// 配方固件类别。
enum RecipeFirmware {
    /// UEFI + ESP。
    Uefi,
    /// BIOS + `biosgrub`。
    Bios,
}

/// 组装 `expert_recipe` 字符串。
fn build_recipe(partitions: &[PartitionSpec], firmware: &RecipeFirmware) -> String {
    let mut lines = vec!["xpxe-custom ::".to_owned()];
    match firmware {
        RecipeFirmware::Uefi => {
            lines.push("512 512 512 fat32 $primary{ } method{ efi } format{ } .".to_owned());
        }
        RecipeFirmware::Bios => {
            lines.push("1 1 1 free $iflabel{ gpt } method{ biosgrub } .".to_owned());
        }
    }
    for part in partitions {
        lines.push(recipe_line(part));
    }
    lines.join(" ")
}

/// 一条分区的 recipe 片段。
fn recipe_line(part: &PartitionSpec) -> String {
    let (min, prio, max) = size_triple(part.size);
    if matches!(part.fs, FsType::Swap) {
        return swap_line(&min, &prio, &max, part.on_lvm);
    }
    format_line(&min, &prio, &max, part)
}

/// swap 分区片段。
fn swap_line(min: &str, prio: &str, max: &str, on_lvm: bool) -> String {
    let lvm = if on_lvm { "$lvmok{ }" } else { "$primary{ }" };
    format!("{min} {prio} {max} linux-swap {lvm} method{{ swap }} format{{ }} .")
}

/// 普通文件系统分区片段。
fn format_line(min: &str, prio: &str, max: &str, part: &PartitionSpec) -> String {
    let fs = part.fs.as_str();
    let lvm = if part.on_lvm {
        "$lvmok{ }"
    } else {
        "$primary{ }"
    };
    let mount = mount_text(&part.mount);
    format!(
        "{min} {prio} {max} {fs} {lvm} method{{ format }} format{{ }} use_filesystem{{ }} filesystem{{ {fs} }} mountpoint{{ {mount} }} ."
    )
}

/// partman 的 min/priority/max。
fn size_triple(size: SizeSpec) -> (String, String, String) {
    match size {
        SizeSpec::Mib { mib } => {
            let text = mib.to_string();
            (text.clone(), text.clone(), text)
        }
        SizeSpec::Rest => ("100".to_owned(), "10000".to_owned(), "-1".to_owned()),
    }
}

/// 挂载点文本。
fn mount_text(mount: &MountPoint) -> String {
    match mount {
        MountPoint::Swap => "swap".to_owned(),
        MountPoint::Path { path } => path.as_str().to_owned(),
    }
}

/// partman 钩子上下文。
#[derive(Serialize)]
struct PartmanContext<'input> {
    /// 选盘脚本 URL。
    disk_url: String,
    /// 失败 POST 前缀。
    fail_prefix: String,
    /// 失败 URL。
    fail_url: String,
    /// 是否自定义分区。
    custom: bool,
    /// UEFI 配方。
    uefi_recipe: &'input str,
    /// BIOS 配方。
    bios_recipe: &'input str,
}

/// 空上下文。
#[derive(Serialize)]
struct Empty;

/// late 钩子上下文。
#[derive(Serialize)]
struct LateContext<'input> {
    /// agent URL。
    agent_url: String,
    /// `sha256sum -c` 行。
    agent_check: String,
    /// plan URL。
    plan_url: String,
    /// plan 校验行。
    plan_check: String,
    /// 单元文件正文。
    unit: &'input str,
    /// finished 事件正文。
    finished: String,
    /// finished URL。
    finished_url: String,
}

/// 渲染 late 钩子。
///
/// # Errors
///
/// 模板失败时返回错误。
fn render_late(request: &RenderRequest, unit: &str) -> Result<String, RenderError> {
    let agent_url = format!("{}/agent", request.session_base());
    let plan_url = format!(
        "{}/p/{}/plan.json",
        request.boot_base().as_str(),
        request.agent_token().as_str()
    );
    render_named(
        "late.sh",
        LATE,
        &LateContext {
            agent_check: format!(
                "{}  /target/usr/local/sbin/xpxe-agent",
                request.agent_sha256()
            ),
            plan_check: format!("{}  /target/var/lib/xpxe/plan.json", request.plan_sha256()),
            agent_url,
            plan_url,
            unit,
            finished: format!(
                "event=installer.finished event_id={}",
                request.event_id().as_str()
            ),
            finished_url: format!("{}/events/installer.finished", request.boot_base().as_str()),
        },
    )
}

/// 选盘脚本上下文。
#[derive(Serialize)]
struct DiskContext {
    /// 最小 GiB。
    min_gib: u64,
    /// 最大 GiB，空表示无上限。
    max_gib: String,
    /// `prefer` 标识。
    prefer: &'static str,
    /// 排除型号 heredoc。
    exclude_block: String,
    /// `include_ok` 函数体。
    include_body: String,
}
