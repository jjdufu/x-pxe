//! `autoexec.ipxe` 快照：DHCP 与静态两种。

use xpxe_domain::{BootToken, HttpUrl, Ipv4Cidr, MacAddr, NodeNetwork};
use xpxe_test_support::{TestResult, ensure_eq};

use super::{AutoexecInput, render_autoexec};

/// DHCP 快照。
const DHCP: &str = include_str!("dhcp.ipxe");
/// 静态快照。
const STATIC: &str = include_str!("static.ipxe");

/// 按网络配置渲染。
///
/// # Errors
///
/// 生成失败时返回错误。
fn render(network: &NodeNetwork) -> TestResult<String> {
    let base = HttpUrl::parse("http://10.77.0.1:8080/")?;
    let token = BootToken::parse("lab-redfish-r1")?;
    Ok(render_autoexec(&AutoexecInput {
        boot_mac: MacAddr::parse("52:54:00:77:01:A1")?,
        network,
        boot_base: &base,
        boot_token: &token,
    })?)
}

/// DHCP：两次 `dhcp` 后链到会话脚本。
///
/// # Errors
///
/// 渲染失败或与快照不符时返回错误。
#[test]
fn dhcp_snapshot() -> TestResult {
    ensure_eq(&render(&NodeNetwork::Dhcp)?.as_str(), &DHCP, "DHCP 快照")?;
    Ok(())
}

/// 静态：地址、掩码、网关、DNS 与 `ifopen`。
///
/// # Errors
///
/// 渲染失败或与快照不符时返回错误。
#[test]
fn static_snapshot() -> TestResult {
    let network = NodeNetwork::Static {
        address: Ipv4Cidr::parse("10.77.0.60/24")?,
        gateway: Some("10.77.0.1".parse()?),
        dns: vec!["10.77.0.1".parse()?, "1.1.1.1".parse()?],
    };
    ensure_eq(&render(&network)?.as_str(), &STATIC, "静态快照")?;
    Ok(())
}
