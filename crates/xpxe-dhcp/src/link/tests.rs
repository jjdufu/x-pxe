//! `link` 模块单元测试：内存传输收发。

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use xpxe_test_support::{TestResult, ensure_eq};

use super::{DhcpTransport as _, MemoryTransport};

/// 内存传输注入与应答。
///
/// # Errors
///
/// 断言失败。
#[test]
fn memory_roundtrip() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (transport, mut peer) = MemoryTransport::pair(67);
        let from = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 68);
        peer.inject(vec![1, 2, 3], from)?;
        let got = transport.recv().await?;
        ensure_eq(&got.payload, &vec![1, 2, 3], "收到注入报文")?;
        ensure_eq(&transport.port(), &67, "端口")?;
        transport.send(&[9], from).await?;
        let reply = peer.next_reply(Duration::from_secs(1)).await;
        ensure_eq(&reply, &Some((vec![9], from)), "取到应答")?;
        let none = peer.next_reply(Duration::from_millis(10)).await;
        ensure_eq(&none, &None, "无更多应答")?;
        Ok(())
    })
}
