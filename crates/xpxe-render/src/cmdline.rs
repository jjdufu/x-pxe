//! 内核参数组装：有序、去重，总长不超过 2048 字节。

use std::net::Ipv4Addr;

use xpxe_domain::{Fqdn, Hostname, Ipv4Cidr, NodeNetwork, NodeSpec};

use crate::RenderError;
use crate::request::RenderRequest;

/// x86 `COMMAND_LINE_SIZE`。
const MAX_CMDLINE: usize = 2048;

/// 组装内核参数。
///
/// # Errors
///
/// 超长或静态地址无法转换为掩码时返回错误。
pub(crate) fn build_cmdline(
    request: &RenderRequest,
    preseed_md5: &str,
) -> Result<String, RenderError> {
    let mut params = Vec::new();
    push_unique(&mut params, "auto=true");
    push_unique(&mut params, "priority=critical");
    push_unique(
        &mut params,
        &format!("preseed/url={}", request.preseed_url()),
    );
    push_unique(&mut params, &format!("preseed/url/checksum={preseed_md5}"));
    let mac = request.node().mac();
    push_unique(&mut params, &format!("BOOTIF=01-{}", mac.to_hexhyp()));
    push_unique(
        &mut params,
        &format!("netcfg/choose_interface={}", mac.to_colon()),
    );
    push_unique(
        &mut params,
        &format!("hostname={}", request.node().hostname()),
    );
    push_unique(
        &mut params,
        &format!("domain={}", domain_of(request.node())),
    );
    push_unique(
        &mut params,
        &format!("locale={}", request.profile().locale()),
    );
    push_unique(
        &mut params,
        &format!("keymap={}", request.profile().keymap()),
    );
    push_static(&mut params, request.node().network())?;
    for console in request.profile().console().items() {
        push_unique(&mut params, &format!("console={}", console.as_str()));
    }
    if let Some(level) = request.profile().debug().debconf_value() {
        push_unique(&mut params, &format!("DEBCONF_DEBUG={level}"));
    }
    let line = params.join(" ");
    if line.len() > max_len() {
        return Err(RenderError::new(
            "cmdline_too_long",
            format!("内核参数 {} 字节，超过 {}", line.len(), max_len()),
        ));
    }
    Ok(line)
}

/// 追加静态网络参数。
///
/// # Errors
///
/// 前缀无法转换为掩码时返回错误。
fn push_static(params: &mut Vec<String>, network: &NodeNetwork) -> Result<(), RenderError> {
    let NodeNetwork::Static {
        address,
        gateway,
        dns,
    } = network
    else {
        return Ok(());
    };
    push_unique(params, "netcfg/disable_autoconfig=true");
    push_unique(
        params,
        &format!("netcfg/get_ipaddress={}", address.address()),
    );
    push_unique(
        params,
        &format!("netcfg/get_netmask={}", netmask_for(*address)?),
    );
    if let Some(gw_addr) = gateway {
        push_unique(params, &format!("netcfg/get_gateway={gw_addr}"));
    }
    if !dns.is_empty() {
        let servers: Vec<String> = dns.iter().map(ToString::to_string).collect();
        push_unique(
            params,
            &format!("netcfg/get_nameservers={}", servers.join(" ")),
        );
    }
    push_unique(params, "netcfg/confirm_static=true");
    Ok(())
}

/// 由前缀长度计算点分掩码。
///
/// # Errors
///
/// 前缀合法时不会失败；保留 Result 以统一错误路径。
pub(crate) fn netmask_for(cidr: Ipv4Cidr) -> Result<Ipv4Addr, RenderError> {
    let prefix = cidr.prefix();
    if prefix == 0 {
        return Ok(Ipv4Addr::UNSPECIFIED);
    }
    if prefix == 32 {
        return Ok(Ipv4Addr::BROADCAST);
    }
    let shift = 32_u32
        .checked_sub(u32::from(prefix))
        .ok_or_else(|| RenderError::new("bad_prefix", "前缀长度无效".to_owned()))?;
    let bits = u32::MAX
        .checked_shl(shift)
        .ok_or_else(|| RenderError::new("bad_prefix", "掩码位移无效".to_owned()))?;
    Ok(Ipv4Addr::from(bits))
}

/// 从 FQDN 取域，否则空字符串。
pub(crate) fn domain_of(node: &NodeSpec) -> String {
    node.fqdn()
        .map_or_else(String::new, |fqdn| domain_suffix(node.hostname(), fqdn))
}

/// 去掉主机名后的域名。
fn domain_suffix(hostname: &Hostname, fqdn: &Fqdn) -> String {
    let prefix = format!("{}.", hostname.as_str());
    fqdn.as_str().strip_prefix(&prefix).unwrap_or("").to_owned()
}

/// 追加且去重（保留首次出现）。
fn push_unique(params: &mut Vec<String>, item: &str) {
    if !params.iter().any(|existing| existing == item) {
        params.push(item.to_owned());
    }
}

/// 内核参数长度上限（供测试）。
#[must_use]
pub(crate) const fn max_len() -> usize {
    MAX_CMDLINE
}
