//! 网卡信息：IPv4 地址、MTU、内核版本自检。

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use socket2::{Domain, Protocol, Socket, Type};

use crate::BootnetError;

/// 网卡的主 IPv4 地址：在绑定到该网卡的 UDP 套接字上 `connect(255.255.255.255)`，
/// 由内核选出的源地址即为网卡地址（不需要 `getifaddrs`）。
///
/// 内核按链路范围选源地址，因此网卡需要有全局范围（scope global）的 IPv4 地址；
/// 只有 host 范围地址的回环网卡会得到其他网卡的地址。
///
/// # Errors
///
/// 网卡不存在、没有 IPv4 地址或权限不足时返回错误。
pub fn iface_ipv4(iface: &str) -> Result<Ipv4Addr, BootnetError> {
    let probe = || -> io::Result<Ipv4Addr> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_broadcast(true)?;
        socket.bind_device(Some(iface.as_bytes()))?;
        socket.connect(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, 67)).into())?;
        let chosen = socket
            .local_addr()?
            .as_socket()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "非 IP 地址"))?;
        match chosen {
            SocketAddr::V4(local) if !local.ip().is_unspecified() => Ok(*local.ip()),
            SocketAddr::V4(_) | SocketAddr::V6(_) => Err(io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "无 IPv4 地址",
            )),
        }
    };
    probe().map_err(|error| socket_error(iface, "取网卡地址", &error))
}

/// 网卡 MTU（读 `/sys/class/net/<iface>/mtu`），失败时按 1500。
#[must_use]
pub fn iface_mtu(iface: &str) -> usize {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/mtu"))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(1500)
}

/// 内核版本是否 ≥ 5.7（此后 `SO_BINDTODEVICE` 不再要求 `CAP_NET_RAW`）。
#[must_use]
pub fn kernel_allows_unprivileged_bind() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .and_then(|release| parse_kernel(&release))
        .is_none_or(|version| version >= (5, 7))
}

/// 解析 `6.12.113+` 这类版本串的主次版本。
#[must_use]
pub fn parse_kernel(release: &str) -> Option<(u32, u32)> {
    let mut parts = release.trim().split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// 把套接字错误翻译为带操作提示的错误。
#[must_use]
pub fn socket_error(iface: &str, action: &str, error: &io::Error) -> BootnetError {
    let kind = error.kind();
    let hint = if kind == io::ErrorKind::PermissionDenied && !kernel_allows_unprivileged_bind() {
        "；内核 < 5.7 时 SO_BINDTODEVICE 需要 CAP_NET_RAW，请在 compose 的 cap_add 中追加 NET_RAW"
    } else if kind == io::ErrorKind::PermissionDenied {
        "；绑定特权端口需要 CAP_NET_BIND_SERVICE"
    } else if kind == io::ErrorKind::AddrInUse {
        "；端口已被占用（宿主机上可能已有 DHCP/TFTP 服务），不会抢占"
    } else {
        ""
    };
    BootnetError::new("socket", format!("{iface} {action}失败：{error}{hint}"))
}

#[cfg(test)]
mod tests;
