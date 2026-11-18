//! 现网 DHCP 探测：从 68 端口以随机本地管理 MAC 发带 `PXEClient` 的 DISCOVER，收集 3 s 内的 OFFER。

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

use xpxe_dhcp::link::DhcpTransport;
use xpxe_dhcp::probe::{classify, discover_packet};
use xpxe_dhcp::wire::Message;
use xpxe_domain::Responder;

use crate::BootnetError;
use crate::answering::PROBE_MAC_PREFIX;

/// 收集 OFFER 的时长。
pub const COLLECT_FOR: Duration = Duration::from_secs(3);

/// 由随机字节生成探测 MAC 与 `xid`：MAC 前 5 字节固定为 [`PROBE_MAC_PREFIX`]，便于本机应答引擎识别并忽略。
#[must_use]
pub const fn probe_identity(seed: u64) -> ([u8; 6], u32) {
    let bytes = seed.to_be_bytes();
    let [p0, p1, p2, p3, p4] = PROBE_MAC_PREFIX;
    let [_, _, _, last, x0, x1, x2, x3] = bytes;
    (
        [p0, p1, p2, p3, p4, last],
        u32::from_be_bytes([x0, x1, x2, x3]),
    )
}

/// 随机种子：取系统时间纳秒与进程号混合（探测 MAC 不需要密码学随机）。
#[must_use]
pub fn seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::from(elapsed.subsec_nanos()) ^ elapsed.as_secs()
        });
    nanos.rotate_left(17) ^ u64::from(std::process::id())
}

/// 发送一次探测并收集应答者（按服务器去重）。
///
/// # Errors
///
/// 发送失败时返回错误；接收错误只结束收集。
pub async fn probe<T: DhcpTransport>(
    transport: &T,
    seed: u64,
    wait: Duration,
) -> Result<Vec<Responder>, BootnetError> {
    let (mac, xid) = probe_identity(seed);
    let packet = discover_packet(mac, xid)
        .encode()
        .map_err(|error| BootnetError::new("probe", format!("编码探测报文失败：{error:?}")))?;
    transport
        .send(&packet, SocketAddrV4::new(Ipv4Addr::BROADCAST, 67))
        .await
        .map_err(|error| BootnetError::new("probe", format!("发送探测报文失败：{error}")))?;
    let deadline = Instant::now()
        .checked_add(wait)
        .unwrap_or_else(Instant::now);
    let mut found: Vec<Responder> = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let Ok(Ok(datagram)) = tokio::time::timeout(remaining, transport.recv()).await else {
            break;
        };
        let Ok(reply) = Message::decode(&datagram.payload) else {
            continue;
        };
        if let Some(responder) = classify(&reply, xid, *datagram.from.ip())
            && !found.iter().any(|known| known.server == responder.server)
        {
            found.push(responder);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
