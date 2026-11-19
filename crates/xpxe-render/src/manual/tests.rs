//! 手动引导菜单渲染：黄金快照、转义与默认项。

use std::fs;
use std::path::PathBuf;

use xpxe_domain::{EntryArch, HttpUrl, MacAddr};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{MenuAction, MenuInput, MenuItem, ipxe_text, render_menu};

/// 测试摘要。
const SHA: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";

/// 构造条目。
fn item(id: &str, title: &str, arch: EntryArch, action: MenuAction) -> MenuItem {
    MenuItem {
        id: id.to_owned(),
        title: title.to_owned(),
        arch,
        action,
    }
}

/// 覆盖四种动作与不可用条目的菜单。
fn items() -> Vec<MenuItem> {
    vec![
        item(
            "pve",
            "Proxmox VE 9.2",
            EntryArch::Any,
            MenuAction::Kernel {
                kernel: SHA.to_owned(),
                initrd: SHA.replace("ab", "cd"),
                cmdline: "ro quiet splash=silent".to_owned(),
            },
        ),
        item(
            "rescue",
            "Rescue ${evil}\nshell",
            EntryArch::Uefi,
            MenuAction::Sanboot {
                iso: SHA.to_owned(),
            },
        ),
        item(
            "old",
            "Legacy only",
            EntryArch::Bios,
            MenuAction::Chain {
                url: "/boot.ipxe".to_owned(),
            },
        ),
        item(
            "broken",
            "Broken",
            EntryArch::Any,
            MenuAction::Unavailable {
                reason: "extract pending".to_owned(),
            },
        ),
        item("local", "Local disk", EntryArch::Any, MenuAction::Exit),
    ]
}

/// 黄金文件比较；`XPXE_UPDATE_GOLDEN=1` 时覆盖。
///
/// # Errors
///
/// 内容不符或读写失败时返回错误。
fn compare_golden(name: &str, actual: &str) -> TestResult {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/render/golden")
        .join(name);
    if std::env::var("XPXE_UPDATE_GOLDEN").is_ok() {
        fs::write(&path, actual)?;
        return Ok(());
    }
    let expected = fs::read_to_string(&path)?;
    ensure_eq(&actual, &expected.as_str(), name)?;
    Ok(())
}

/// 快照与注入防护。
///
/// # Errors
///
/// 渲染失败或断言失败时返回错误。
#[test]
fn snapshot_and_escaping() -> TestResult {
    let base = HttpUrl::parse("http://10.77.0.1:8080/")?;
    let entries = items();
    let input = MenuInput {
        boot_base: &base,
        mac: MacAddr::parse("52:54:00:77:14:01")?,
        timeout_s: 15,
        default_entry: Some("pve"),
        items: &entries,
    };
    let script = render_menu(&input)?;
    compare_golden("manual-menu.ipxe", &script)?;
    ensure(!script.contains("${evil}"), "标题中的 ${} 不得展开")?;
    ensure(
        script.contains("item m-rescue Rescue ＄{evil}shell"),
        "控制字符被去掉",
    )?;
    ensure(
        script.contains("chain http://10.77.0.1:8080/boot.ipxe"),
        "同源 chain 补全",
    )?;
    ensure(
        script.contains("choose --timeout 15000 --default m-pve target"),
        "倒计时与默认项",
    )?;
    ensure_eq(&render_menu(&input)?, &script, "渲染确定")?;
    Ok(())
}

/// 默认项不在条目中时省略；倒计时 0 时不加 --timeout。
///
/// # Errors
///
/// 渲染失败或断言失败时返回错误。
#[test]
fn default_and_timeout_optional() -> TestResult {
    let base = HttpUrl::parse("http://10.77.0.1:8080")?;
    let input = MenuInput {
        boot_base: &base,
        mac: MacAddr::parse("52:54:00:77:14:01")?,
        timeout_s: 0,
        default_entry: Some("gone"),
        items: &[],
    };
    let script = render_menu(&input)?;
    ensure(
        script.contains("\nchoose target || goto xpxe-exit\n"),
        &script,
    )?;
    ensure_eq(&ipxe_text("  a$b\t "), &"a＄b".to_owned(), "转义")?;
    Ok(())
}
