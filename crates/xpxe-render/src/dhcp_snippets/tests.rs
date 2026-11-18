//! 配置片段：四种服务器都写出本机地址、三个固件名与两个 URL，iPXE 判断排在最前。

use std::net::Ipv4Addr;

use xpxe_domain::HttpUrl;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{ServerKind, render_snippets};

/// 每段都包含关键值，且 iPXE 分支先于固件分支。
///
/// # Errors
///
/// 断言失败。
#[test]
fn snippets_cover_all_targets() -> TestResult {
    let base = HttpUrl::parse("http://192.168.2.3:8080/")?;
    let list = render_snippets(Ipv4Addr::new(192, 168, 2, 3), &base);
    ensure_eq(&list.len(), &4, "四种服务器")?;
    for snippet in &list {
        let text = snippet.text.as_str();
        ensure(
            text.contains("http://192.168.2.3:8080/boot.ipxe"),
            "iPXE 脚本 URL（基址末尾 / 去重）",
        )?;
        ensure(
            text.contains("http://192.168.2.3:8080/fw/x64/ipxe.efi"),
            "HTTP Boot URL",
        )?;
        ensure(text.contains("192.168.2.3"), "next-server")?;
        ensure(
            text.contains("ipxe.efi") && text.contains("undionly.kpxe"),
            "固件名",
        )?;
        let ipxe = text.find("boot.ipxe").ok_or("缺 iPXE 分支")?;
        let efi = text
            .find("\"ipxe.efi\"")
            .or_else(|| text.find("'ipxe.efi'"))
            .ok_or("缺 UEFI 分支")?;
        ensure(ipxe < efi, "iPXE 分支必须在固件分支之前")?;
    }
    ensure_eq(
        &list
            .iter()
            .map(|snippet| snippet.kind.as_str())
            .collect::<Vec<_>>(),
        &vec!["kea", "isc_dhcpd", "windows_dhcp", "routeros"],
        "顺序",
    )?;
    let kea = list
        .iter()
        .find(|snippet| snippet.kind == ServerKind::Kea)
        .ok_or("kea")?;
    let wrapped = format!("{{{}}}", kea.text);
    let parsed: serde_json::Value = serde_json::from_str(&wrapped)?;
    ensure_eq(
        &parsed
            .pointer("/client-classes/3/boot-file-name")
            .and_then(serde_json::Value::as_str),
        &Some("undionly.kpxe"),
        "Kea 片段是合法 JSON",
    )?;
    Ok(())
}
