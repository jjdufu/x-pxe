//! 基于 tokio 的只读 TFTP 传输层。

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;

use crate::packet::{ErrorCode, Packet};
use crate::session::{Action, Limits, Session};

/// 单个 UDP 报文的接收缓冲大小。
const RECV_BUF: usize = 65536;

/// 文件来源。
pub trait Source: Send + Sync {
    /// 按规范化后的相对路径打开文件；不存在返回 `None`。
    fn open(&self, path: &str) -> Option<Arc<[u8]>>;
}

/// 规范化客户端请求的文件名：去掉前导 `/`，`\` 视为分隔符，拒绝 `..` 与空路径。
#[must_use]
pub fn normalize(filename: &str) -> Option<String> {
    let unified = filename.replace('\\', "/");
    let mut parts = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// 一次传输的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// 对端地址。
    pub peer: SocketAddr,
    /// 规范化后的文件名（无法解析时为原始名）。
    pub path: String,
    /// 是否完整传完。
    pub completed: bool,
    /// 协商的块大小。
    pub blksize: usize,
    /// 协商的窗口。
    pub window: u16,
}

/// 发送一组报文。
///
/// # Errors
///
/// 套接字错误。
async fn send_all(socket: &UdpSocket, peer: SocketAddr, packets: &[Packet]) -> io::Result<()> {
    for packet in packets {
        let _sent = socket.send_to(&packet.encode(), peer).await?;
    }
    Ok(())
}

/// 为单个读请求完成传输。
///
/// # Errors
///
/// 套接字错误。
pub async fn transfer(
    socket: &UdpSocket,
    peer: SocketAddr,
    data: Arc<[u8]>,
    options: &[(String, String)],
    limits: Limits,
) -> io::Result<(bool, usize, u16)> {
    let mut session = Session::new(data, options, limits);
    let mut action = session.start();
    let mut buf = vec![0_u8; RECV_BUF];
    loop {
        match action {
            Action::Send(packets) => send_all(socket, peer, &packets).await?,
            Action::Wait => {}
            Action::Done => return Ok((true, session.blksize(), session.window())),
            Action::Abort(packet) => {
                if let Some(error) = packet {
                    send_all(socket, peer, &[error]).await?;
                }
                return Ok((false, session.blksize(), session.window()));
            }
        }
        action = match tokio::time::timeout(session.timeout(), socket.recv_from(&mut buf)).await {
            Err(_elapsed) => session.on_timeout(),
            Ok(received) => {
                let (len, from) = received?;
                let payload = buf.get(..len).unwrap_or_default();
                if from == peer {
                    Packet::parse(payload).map_or(Action::Wait, |packet| session.on_packet(&packet))
                } else {
                    let stray = Packet::Error {
                        code: ErrorCode::UnknownTransfer,
                        message: "unknown transfer id".to_owned(),
                    };
                    send_all(socket, from, &[stray]).await?;
                    Action::Wait
                }
            }
        };
    }
}

/// 向对端回错误并给出失败结果。
///
/// # Errors
///
/// 套接字错误。
async fn refuse(
    socket: &UdpSocket,
    peer: SocketAddr,
    path: String,
    code: ErrorCode,
    message: &str,
) -> io::Result<Outcome> {
    let error = Packet::Error {
        code,
        message: message.to_owned(),
    };
    send_all(socket, peer, &[error]).await?;
    Ok(Outcome {
        peer,
        path,
        completed: false,
        blksize: 0,
        window: 0,
    })
}

/// 处理一个首包：读请求则建立临时端口完成传输，其他请求回错误。
///
/// # Errors
///
/// 套接字错误。
async fn handle(
    bind: SocketAddr,
    peer: SocketAddr,
    request: Packet,
    source: Arc<dyn Source>,
    limits: Limits,
) -> io::Result<Outcome> {
    let socket = UdpSocket::bind(SocketAddr::new(bind.ip(), 0)).await?;
    let Packet::Read {
        filename,
        mode,
        options,
    } = request
    else {
        return refuse(
            &socket,
            peer,
            String::new(),
            ErrorCode::AccessViolation,
            "read-only server",
        )
        .await;
    };
    if mode != "octet" && mode != "netascii" {
        return refuse(
            &socket,
            peer,
            filename,
            ErrorCode::IllegalOperation,
            "unsupported mode",
        )
        .await;
    }
    let Some(path) = normalize(&filename) else {
        return refuse(
            &socket,
            peer,
            filename,
            ErrorCode::AccessViolation,
            "bad path",
        )
        .await;
    };
    let Some(data) = source.open(&path) else {
        return refuse(
            &socket,
            peer,
            path,
            ErrorCode::FileNotFound,
            "file not found",
        )
        .await;
    };
    let (completed, blksize, window) = transfer(&socket, peer, data, &options, limits).await?;
    Ok(Outcome {
        peer,
        path,
        completed,
        blksize,
        window,
    })
}

/// 在已绑定的套接字上持续服务，直到套接字出错。
///
/// # Errors
///
/// 监听套接字错误。
pub async fn serve(socket: UdpSocket, source: Arc<dyn Source>, limits: Limits) -> io::Result<()> {
    let bind = socket.local_addr()?;
    let mut buf = vec![0_u8; RECV_BUF];
    loop {
        let (len, peer) = socket.recv_from(&mut buf).await?;
        let Ok(request) = Packet::parse(buf.get(..len).unwrap_or_default()) else {
            continue;
        };
        let task_source = Arc::clone(&source);
        drop(tokio::spawn(async move {
            match handle(bind, peer, request, task_source, limits).await {
                Ok(outcome) => tracing::info!(
                    peer = %outcome.peer,
                    path = %outcome.path,
                    completed = outcome.completed,
                    blksize = outcome.blksize,
                    window = outcome.window,
                    "tftp transfer"
                ),
                Err(error) => tracing::warn!(%peer, %error, "tftp transfer failed"),
            }
        }));
    }
}

#[cfg(test)]
mod tests;
