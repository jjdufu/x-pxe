//! DHCP 报文收发抽象：[`DhcpTransport`]，以及 UDP 实现与测试用内存实现。

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, mpsc};

/// 接收缓冲：大于 [`crate::wire::MAX_PACKET_LEN`]，以便识别并丢弃超长报文。
const RECV_BUF: usize = 2048;

/// 收到的一个报文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    /// 报文字节。
    pub payload: Vec<u8>,
    /// 源地址。
    pub from: SocketAddrV4,
}

/// DHCP 报文收发。
pub trait DhcpTransport: Send + Sync {
    /// 本地监听端口（67、4011 或 68）。
    fn port(&self) -> u16;

    /// 接收下一个 IPv4 报文。
    ///
    /// # Errors
    ///
    /// 底层套接字错误或通道关闭。
    fn recv(&self) -> impl Future<Output = io::Result<Datagram>> + Send;

    /// 发送报文。
    ///
    /// # Errors
    ///
    /// 底层套接字错误或通道关闭。
    fn send(&self, payload: &[u8], to: SocketAddrV4)
    -> impl Future<Output = io::Result<()>> + Send;
}

/// 绑定到网卡的 UDP 套接字。
#[derive(Debug)]
pub struct UdpTransport {
    /// 套接字。
    socket: UdpSocket,
    /// 本地端口。
    port: u16,
}

impl UdpTransport {
    /// 在 `0.0.0.0:<port>` 上建立套接字：`SO_REUSEADDR`、`SO_BROADCAST`、`SO_BINDTODEVICE(iface)`。
    ///
    /// 绑定到网卡后，收包网卡与广播出口都限定为 `iface`，因此不需要 `IP_PKTINFO`。
    ///
    /// # Errors
    ///
    /// 任一套接字调用失败（端口被占用时为 `AddrInUse`；内核 < 5.7 且无 `CAP_NET_RAW` 时
    /// `SO_BINDTODEVICE` 返回 `PermissionDenied`）。
    pub fn bind(iface: &str, port: u16) -> io::Result<Self> {
        let socket = bind_udp(iface, Ipv4Addr::UNSPECIFIED, port)?;
        Ok(Self { socket, port })
    }
}

/// 建立绑定到网卡的非阻塞 UDP 套接字。
///
/// # Errors
///
/// 任一套接字调用失败。
pub fn bind_udp(iface: &str, ip: Ipv4Addr, port: u16) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_broadcast(true)?;
    socket.bind_device(Some(iface.as_bytes()))?;
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(ip, port)).into())?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

impl DhcpTransport for UdpTransport {
    fn port(&self) -> u16 {
        self.port
    }

    async fn recv(&self) -> io::Result<Datagram> {
        let mut buf = vec![0_u8; RECV_BUF];
        loop {
            let (len, source) = self.socket.recv_from(&mut buf).await?;
            if let SocketAddr::V4(from) = source {
                buf.truncate(len);
                return Ok(Datagram { payload: buf, from });
            }
        }
    }

    async fn send(&self, payload: &[u8], to: SocketAddrV4) -> io::Result<()> {
        let _sent: usize = self.socket.send_to(payload, SocketAddr::V4(to)).await?;
        Ok(())
    }
}

/// 内存传输：测试中代替真实套接字。
#[derive(Debug)]
pub struct MemoryTransport {
    /// 本地端口。
    port: u16,
    /// 入站报文。
    inbound: Mutex<mpsc::UnboundedReceiver<Datagram>>,
    /// 出站报文。
    outbound: mpsc::UnboundedSender<(Vec<u8>, SocketAddrV4)>,
}

/// [`MemoryTransport`] 的对端：注入请求、读取应答。
#[derive(Debug)]
pub struct MemoryPeer {
    /// 注入端。
    inject: mpsc::UnboundedSender<Datagram>,
    /// 应答。
    replies: mpsc::UnboundedReceiver<(Vec<u8>, SocketAddrV4)>,
}

impl MemoryTransport {
    /// 建立一对内存传输与对端。
    #[must_use]
    pub fn pair(port: u16) -> (Self, MemoryPeer) {
        let (inject, inbound) = mpsc::unbounded_channel();
        let (outbound, replies) = mpsc::unbounded_channel();
        (
            Self {
                port,
                inbound: Mutex::new(inbound),
                outbound,
            },
            MemoryPeer { inject, replies },
        )
    }
}

impl DhcpTransport for MemoryTransport {
    fn port(&self) -> u16 {
        self.port
    }

    async fn recv(&self) -> io::Result<Datagram> {
        self.inbound
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "memory transport closed"))
    }

    fn send(
        &self,
        payload: &[u8],
        to: SocketAddrV4,
    ) -> impl Future<Output = io::Result<()>> + Send {
        let sent = self
            .outbound
            .send((payload.to_vec(), to))
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error.to_string()));
        std::future::ready(sent)
    }
}

impl MemoryPeer {
    /// 注入一个入站报文。
    ///
    /// # Errors
    ///
    /// 传输端已关闭。
    pub fn inject(&self, payload: Vec<u8>, from: SocketAddrV4) -> io::Result<()> {
        self.inject
            .send(Datagram { payload, from })
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error.to_string()))
    }

    /// 取下一个应答；`wait` 内没有则返回 `None`。
    pub async fn next_reply(
        &mut self,
        wait: std::time::Duration,
    ) -> Option<(Vec<u8>, SocketAddrV4)> {
        tokio::time::timeout(wait, self.replies.recv())
            .await
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
