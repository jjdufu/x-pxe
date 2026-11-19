//! 手动引导菜单校验样例。

use serde_json::json;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{BootMenu, EntryArch, EntryKind};
use crate::MacAddr;
use crate::versioning::ConfigDocument as _;

/// 测试用 ISO 摘要。
const ISO: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";

/// 一份合法菜单。
///
/// # Errors
///
/// 解码失败时返回错误。
fn sample() -> TestResult<BootMenu> {
    Ok(serde_json::from_value(json!({
        "enabled": true,
        "allow_macs": ["52:54:00:77:14:01"],
        "default_entry": "pve",
        "timeout_s": 10,
        "entries": [
            {"id": "pve", "title": "Proxmox VE 9.2", "kind": {"type": "kernel_initrd", "iso": ISO,
              "kernel_path": "/boot/linux26", "initrd_path": "/boot/initrd.img", "cmdline": "ro quiet splash=silent", "embed_iso": true}},
            {"id": "rescue", "title": "救援盘", "arch": "uefi", "kind": {"type": "sanboot", "iso": ISO}},
            {"id": "next", "title": "链到其他脚本", "kind": {"type": "chain", "url": "/boot.ipxe"}},
            {"id": "local", "title": "本地启动", "enabled": false, "kind": {"type": "exit"}}
        ]
    }))?)
}

/// 校验失败时的错误码列表。
fn codes(menu: &BootMenu) -> Vec<String> {
    menu.validate()
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|error| format!("{}:{}", error.path, error.code))
        .collect()
}

/// 合法菜单与辅助方法。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn valid_menu_and_helpers() -> TestResult {
    let menu = sample()?;
    ensure(menu.validate().is_ok(), "合法菜单通过校验")?;
    let mac = MacAddr::parse("52:54:00:77:14:01")?;
    let other = MacAddr::parse("52:54:00:77:14:02")?;
    ensure(menu.shows_for(mac, true), "白名单命中")?;
    ensure(
        !menu.shows_for(other, false),
        "未开启 show_unknown 时未知机器不显示",
    )?;
    let open = BootMenu {
        show_unknown: true,
        ..menu.clone()
    };
    ensure(open.shows_for(other, false), "show_unknown 对未知机器显示")?;
    ensure(
        !open.shows_for(other, true),
        "已登记机器不受 show_unknown 影响",
    )?;
    let closed = BootMenu {
        enabled: false,
        ..menu.clone()
    };
    ensure(!closed.shows_for(mac, true), "总开关关闭时不显示")?;
    ensure(menu.entry("local").is_none(), "停用条目不可选")?;
    ensure_eq(
        &menu.referencing(&crate::Sha256Hex::parse(ISO)?),
        &vec!["pve".to_owned(), "rescue".to_owned()],
        "引用 ISO 的条目",
    )?;
    ensure(
        EntryArch::Uefi.matches(true) && !EntryArch::Uefi.matches(false),
        "arch 匹配",
    )?;
    ensure(
        BootMenu::default().validate().is_ok() && !BootMenu::default().enabled,
        "默认关闭且合法",
    )?;
    Ok(())
}

/// 非法字段逐项报告。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn rejects_bad_fields() -> TestResult {
    let mut menu = sample()?;
    menu.timeout_s = 99_999;
    menu.default_entry = Some("local".to_owned());
    menu.chain_prefixes = vec!["ftp://x".to_owned()];
    if let Some(first) = menu.entries.first_mut() {
        first.kind = EntryKind::KernelInitrd {
            iso: crate::Sha256Hex::parse(ISO)?,
            kernel_path: "/boot/../etc/shadow".to_owned(),
            initrd_path: "boot/initrd".to_owned(),
            cmdline: "quiet; reboot $x".to_owned(),
            embed_iso: false,
            iso_name: Some("/a b.iso".to_owned()),
        };
    }
    if let Some(chain) = menu.entries.get_mut(2) {
        chain.kind = EntryKind::Chain {
            url: "http://evil.example/x.ipxe".to_owned(),
        };
        chain.id = "pve".to_owned();
        chain.title = String::new();
    }
    let found = codes(&menu);
    for code in [
        "/timeout_s:out_of_range",
        "/default_entry:unknown_entry",
        "/chain_prefixes/0:bad_prefix",
        "/entries/0/kind/kernel_path:bad_path",
        "/entries/0/kind/initrd_path:bad_path",
        "/entries/0/kind/iso_name:bad_path",
        "/entries/0/kind/cmdline:bad_cmdline",
        "/entries/2/kind/url:chain_not_allowed",
        "/entries/2/id:duplicate_id",
        "/entries/2/title:bad_title",
    ] {
        ensure(
            found.iter().any(|item| item == code),
            &format!("应报告 {code}：{found:?}"),
        )?;
    }
    Ok(())
}

/// chain 白名单与未知字段。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn chain_whitelist_and_unknown_fields() -> TestResult {
    let mut menu = sample()?;
    menu.chain_prefixes = vec!["http://10.77.0.1:8080/".to_owned()];
    if let Some(chain) = menu.entries.get_mut(2) {
        chain.kind = EntryKind::Chain {
            url: "http://10.77.0.1:8080/x.ipxe".to_owned(),
        };
    }
    ensure(menu.validate().is_ok(), "白名单前缀通过")?;
    let bad = serde_json::from_value::<BootMenu>(json!({"entries": [], "surprise": 1_u8}));
    ensure(bad.is_err(), "拒绝未知字段")?;
    let migrated = BootMenu::migrate(1, serde_json::to_value(&menu)?)?;
    ensure_eq(&migrated, &menu, "schema 1 往返")?;
    Ok(())
}
