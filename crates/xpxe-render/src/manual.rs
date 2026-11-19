//! 手动引导菜单脚本 `/m/menu.ipxe`（Step 14）。
//!
//! 条目按菜单顺序渲染；`arch` 限定的条目由 iPXE 运行时 `${platform}` 过滤。
//! 标题与原因经 [`ipxe_text`] 转义，URL 与 cmdline 已由领域层白名单校验。

use serde::Serialize;
use xpxe_domain::{EntryArch, HttpUrl, MacAddr};

use crate::RenderError;
use crate::template::render_named;

/// 模板源码。
const TEMPLATE: &str = include_str!("templates/manual-menu.ipxe.j2");

/// 已解析的条目动作（抽取产物与 URL 已由控制面就绪）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MenuAction {
    /// 整盘 ISO 映射。
    Sanboot {
        /// ISO SHA-256。
        iso: String,
    },
    /// 内核 + initrd（产物 SHA-256）。
    Kernel {
        /// 内核产物。
        kernel: String,
        /// initrd 产物（可能已嵌入 ISO）。
        initrd: String,
        /// 内核命令行。
        cmdline: String,
    },
    /// 链到脚本（绝对 URL）。
    Chain {
        /// URL。
        url: String,
    },
    /// 本地启动。
    Exit,
    /// 暂不可用（例如抽取未完成或 ISO 已删除）。
    Unavailable {
        /// 原因。
        reason: String,
    },
}

/// 一个待渲染条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// 条目 ID。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 适用固件。
    pub arch: EntryArch,
    /// 动作。
    pub action: MenuAction,
}

/// 渲染输入。
#[derive(Debug, Clone, Copy)]
pub struct MenuInput<'input> {
    /// 引导端 HTTP 根。
    pub boot_base: &'input HttpUrl,
    /// 请求菜单的 MAC。
    pub mac: MacAddr,
    /// 倒计时秒数（0 为不限）。
    pub timeout_s: u32,
    /// 默认条目 ID。
    pub default_entry: Option<&'input str>,
    /// 已启用的条目（按顺序）。
    pub items: &'input [MenuItem],
}

/// 模板条目。
#[derive(Serialize)]
struct ItemContext<'input> {
    /// ID。
    id: &'input str,
    /// 转义后的标题。
    title: String,
    /// `any`/`bios`/`uefi`。
    arch: &'static str,
    /// 动作。
    action: MenuAction,
}

/// 模板上下文。
#[derive(Serialize)]
struct Context<'input> {
    /// HTTP 根（去掉末尾斜杠）。
    boot_base: &'input str,
    /// MAC。
    mac: String,
    /// 倒计时毫秒。
    timeout_ms: u64,
    /// 默认条目。
    default: Option<&'input str>,
    /// 条目。
    items: Vec<ItemContext<'input>>,
}

/// 把任意文本变成可安全放进 iPXE 命令行尾部的单行文本：
/// 去掉控制字符，`$` 换成全角 `＄`（避免 `${...}` 展开），首尾空白去除。
#[must_use]
pub fn ipxe_text(raw: &str) -> String {
    raw.chars()
        .filter(|item| !item.is_control())
        .map(|item| if item == '$' { '＄' } else { item })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// 动作中的自由文本同样转义；同源 chain 路径补全为绝对 URL。
fn finish_action(action: &MenuAction, base: &str) -> MenuAction {
    match action {
        MenuAction::Unavailable { reason } => MenuAction::Unavailable {
            reason: ipxe_text(reason),
        },
        MenuAction::Chain { url } if url.starts_with('/') => MenuAction::Chain {
            url: format!("{base}{url}"),
        },
        MenuAction::Sanboot { .. }
        | MenuAction::Kernel { .. }
        | MenuAction::Chain { .. }
        | MenuAction::Exit => action.clone(),
    }
}

/// 渲染手动引导菜单。
///
/// # Errors
///
/// 模板渲染失败时返回错误。
pub fn render_menu(input: &MenuInput<'_>) -> Result<String, RenderError> {
    let base = input.boot_base.as_str().trim_end_matches('/');
    let items = input
        .items
        .iter()
        .map(|item| ItemContext {
            id: &item.id,
            title: ipxe_text(&item.title),
            arch: match item.arch {
                EntryArch::Any => "any",
                EntryArch::Bios => "bios",
                EntryArch::Uefi => "uefi",
            },
            action: finish_action(&item.action, base),
        })
        .collect();
    let default = input
        .default_entry
        .filter(|id| input.items.iter().any(|item| item.id == *id));
    render_named(
        "manual-menu.ipxe",
        TEMPLATE,
        &Context {
            boot_base: base,
            mac: input.mac.to_colon(),
            timeout_ms: u64::from(input.timeout_s).saturating_mul(1000),
            default,
            items,
        },
    )
}

#[cfg(test)]
mod tests;
