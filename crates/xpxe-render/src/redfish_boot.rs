//! Redfish 引导 ISO 内的 `autoexec.ipxe`：按 `boot_mac` 选网卡，DHCP 或静态配置后链到会话脚本。

use serde::Serialize;
use xpxe_domain::{BootToken, HttpUrl, MacAddr, NodeNetwork};

use crate::RenderError;
use crate::cmdline::netmask_for;
use crate::template::render_named;

/// 模板源码。
const TEMPLATE: &str = include_str!("templates/autoexec.ipxe.j2");

/// 渲染 `autoexec.ipxe` 所需的输入。
#[derive(Debug, Clone, Copy)]
pub struct AutoexecInput<'input> {
    /// 用于引导的网卡 MAC（节点规格中的 `mac`）。
    pub boot_mac: MacAddr,
    /// 节点网络配置。
    pub network: &'input NodeNetwork,
    /// HTTP 根，例如 `http://10.77.0.1:8080`。
    pub boot_base: &'input HttpUrl,
    /// 会话令牌。
    pub boot_token: &'input BootToken,
}

/// 模板上下文。
#[derive(Serialize)]
struct Context<'input> {
    /// MAC（冒号小写）。
    boot_mac: String,
    /// 是否静态地址。
    #[serde(rename = "static")]
    is_static: bool,
    /// 静态地址。
    ip: String,
    /// 掩码。
    netmask: String,
    /// 网关（可空）。
    gateway: String,
    /// 第一个 DNS（可空）。
    dns: String,
    /// HTTP 根（去掉末尾斜杠）。
    boot_base: &'input str,
    /// 令牌。
    boot_token: &'input str,
}

/// 渲染 `autoexec.ipxe`。
///
/// # Errors
///
/// 模板渲染失败时返回错误。
pub fn render_autoexec(input: &AutoexecInput<'_>) -> Result<String, RenderError> {
    let mut context = Context {
        boot_mac: input.boot_mac.to_colon(),
        is_static: false,
        ip: String::new(),
        netmask: String::new(),
        gateway: String::new(),
        dns: String::new(),
        boot_base: input.boot_base.as_str().trim_end_matches('/'),
        boot_token: input.boot_token.as_str(),
    };
    if let NodeNetwork::Static {
        address,
        gateway,
        dns,
    } = input.network
    {
        context.is_static = true;
        context.ip = address.address().to_string();
        context.netmask = netmask_for(*address)?.to_string();
        context.gateway = gateway.map_or_else(String::new, |gw| gw.to_string());
        context.dns = dns.first().map_or_else(String::new, ToString::to_string);
    }
    // minijinja 默认去掉模板末尾换行，这里补回，保证脚本以换行结尾
    let mut script = render_named("autoexec.ipxe", TEMPLATE, &context)?;
    script.push('\n');
    Ok(script)
}

#[cfg(test)]
mod tests;
