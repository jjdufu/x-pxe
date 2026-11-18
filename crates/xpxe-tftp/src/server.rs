//! 基于 tokio 的只读 TFTP 传输层。

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// 已被确认的字节数。
    pub bytes: u64,
    /// 耗时。
    pub elapsed: Duration,
}

/// 传输观测回调。
pub trait Observer: Send + Sync {
    /// 一次读传输开始（文件已找到、准入通过）。
    fn started(&self, peer: SocketAddr, path: &str);
    /// 一次传输结束（完成、失败或被拒绝）。
    fn finished(&self, outcome: &Outcome);
}

/// 把传输结果写成 `tracing` 日志的观测器。
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingObserver;

impl Observer for TracingObserver {
    fn started(&self, peer: SocketAddr, path: &str) {
        tracing::debug!(%peer, path, "tftp start");
    }

    fn finished(&self, outcome: &Outcome) {
        tracing::info!(
            peer = %outcome.peer,
            path = %outcome.path,
            completed = outcome.completed,
            blksize = outcome.blksize,
            window = outcome.window,
            bytes = outcome.bytes,
            "tftp transfer"
        );
    }
}

/// 服务参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServeConfig {
    /// 协商上限。
    pub limits: Limits,
    /// 每个传输的临时套接字绑定的地址（网卡地址）。
    pub transfer_ip: IpAddr,
    /// 全局并发上限。
    pub max_total: usize,
    /// 每个源 IP 的并发上限。
    pub max_per_client: usize,
}

impl ServeConfig {
    /// 计划默认值：全局 64、每源 IP 4。
    #[must_use]
    pub fn new(transfer_ip: IpAddr) -> Self {
        Self {
            limits: Limits::default(),
            transfer_ip,
            max_total: 64,
            max_per_client: 4,
        }
    }
}

/// 并发准入表。
#[derive(Debug, Default)]
struct Admission {
    /// 当前传输总数。
    total: usize,
    /// 每源 IP 的传输数。
    per_client: HashMap<IpAddr, usize>,
}

/// 持有一个准入名额，析构时归还。
#[derive(Debug)]
struct Permit {
    /// 准入表。
    table: Arc<Mutex<Admission>>,
    /// 源 IP。
    client: IpAddr,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut table) = self.table.lock() {
            table.total = table.total.saturating_sub(1);
            let remove = table.per_client.get_mut(&self.client).is_some_and(|count| {
                *count = count.saturating_sub(1);
                *count == 0
            });
            if remove {
                let _gone: Option<usize> = table.per_client.remove(&self.client);
            }
        }
    }
}

/// 尝试取得一个准入名额。
fn admit(table: &Arc<Mutex<Admission>>, client: IpAddr, config: &ServeConfig) -> Option<Permit> {
    let mut guard = table.lock().ok()?;
    let current = guard.per_client.get(&client).copied().unwrap_or(0);
    if guard.total >= config.max_total || current >= config.max_per_client {
        return None;
    }
    guard.total = guard.total.saturating_add(1);
    let _previous: Option<usize> = guard.per_client.insert(client, current.saturating_add(1));
    drop(guard);
    Some(Permit {
        table: Arc::clone(table),
        client,
    })
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

/// 一次传输的协商结果与进度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// 是否完整传完。
    pub completed: bool,
    /// 协商的块大小。
    pub blksize: usize,
    /// 协商的窗口。
    pub window: u16,
    /// 已被确认的字节数。
    pub bytes: u64,
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
) -> io::Result<Progress> {
    let mut session = Session::new(data, options, limits);
    let mut action = session.start();
    let mut buf = vec![0_u8; RECV_BUF];
    loop {
        match action {
            Action::Send(packets) => send_all(socket, peer, &packets).await?,
            Action::Wait => {}
            Action::Done => return Ok(progress(&session, true)),
            Action::Abort(packet) => {
                if let Some(error) = packet {
                    send_all(socket, peer, &[error]).await?;
                }
                return Ok(progress(&session, false));
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

/// 会话当前进度。
fn progress(session: &Session, completed: bool) -> Progress {
    Progress {
        completed,
        blksize: session.blksize(),
        window: session.window(),
        bytes: session.acked_bytes(),
    }
}

/// 一个待处理的首包。
#[derive(Debug)]
struct Request {
    /// 对端。
    peer: SocketAddr,
    /// 首包。
    packet: Packet,
    /// 收到时间。
    started: Instant,
}

impl Request {
    /// 失败结果。
    fn failed(&self, path: String) -> Outcome {
        Outcome {
            peer: self.peer,
            path,
            completed: false,
            blksize: 0,
            window: 0,
            bytes: 0,
            elapsed: self.started.elapsed(),
        }
    }
}

/// 向对端回错误并给出失败结果。
///
/// # Errors
///
/// 套接字错误。
async fn refuse(
    socket: &UdpSocket,
    request: &Request,
    path: String,
    code: ErrorCode,
    message: &str,
) -> io::Result<Outcome> {
    let error = Packet::Error {
        code,
        message: message.to_owned(),
    };
    send_all(socket, request.peer, &[error]).await?;
    Ok(request.failed(path))
}

/// 拒绝原因：文件名、错误码、说明。
type Refusal = (String, ErrorCode, &'static str);
/// 找到的文件：规范化路径与内容。
type Found = (String, Arc<[u8]>);

/// 校验首包并查找文件。
///
/// # Errors
///
/// 非读请求、非 `octet` 模式、非法路径或文件不存在时返回拒绝原因。
fn resolve(packet: &Packet, source: &dyn Source) -> Result<Found, Refusal> {
    let Packet::Read { filename, mode, .. } = packet else {
        return Err((
            String::new(),
            ErrorCode::AccessViolation,
            "read-only server",
        ));
    };
    if !mode.eq_ignore_ascii_case("octet") {
        return Err((
            filename.clone(),
            ErrorCode::IllegalOperation,
            "only octet mode",
        ));
    }
    let Some(path) = normalize(filename) else {
        return Err((filename.clone(), ErrorCode::AccessViolation, "bad path"));
    };
    match source.open(&path) {
        Some(data) => Ok((path, data)),
        None => Err((path, ErrorCode::FileNotFound, "file not found")),
    }
}

/// 处理一个首包：读请求则在临时端口完成传输，其他请求回错误。
///
/// # Errors
///
/// 套接字错误。
async fn handle(
    request: Request,
    source: Arc<dyn Source>,
    config: ServeConfig,
    admission: Arc<Mutex<Admission>>,
    observer: Arc<dyn Observer>,
) -> io::Result<Outcome> {
    let socket = UdpSocket::bind(SocketAddr::new(config.transfer_ip, 0)).await?;
    let (path, data) = match resolve(&request.packet, source.as_ref()) {
        Ok(found) => found,
        Err((path, code, message)) => return refuse(&socket, &request, path, code, message).await,
    };
    let Some(permit) = admit(&admission, request.peer.ip(), &config) else {
        return refuse(&socket, &request, path, ErrorCode::Undefined, "server busy").await;
    };
    observer.started(request.peer, &path);
    let options = match &request.packet {
        Packet::Read { options, .. } => options.as_slice(),
        Packet::Write
        | Packet::Data { .. }
        | Packet::Ack { .. }
        | Packet::Error { .. }
        | Packet::OptionAck { .. } => &[],
    };
    let result = transfer(&socket, request.peer, data, options, config.limits).await;
    drop(permit);
    let done = result?;
    Ok(Outcome {
        peer: request.peer,
        path,
        completed: done.completed,
        blksize: done.blksize,
        window: done.window,
        bytes: done.bytes,
        elapsed: request.started.elapsed(),
    })
}

/// 在已绑定的监听套接字上持续服务，直到套接字出错。
///
/// # Errors
///
/// 监听套接字错误。
pub async fn serve(
    socket: UdpSocket,
    source: Arc<dyn Source>,
    config: ServeConfig,
    observer: Arc<dyn Observer>,
) -> io::Result<()> {
    let admission = Arc::new(Mutex::new(Admission::default()));
    let mut buf = vec![0_u8; RECV_BUF];
    loop {
        let (len, peer) = socket.recv_from(&mut buf).await?;
        let Ok(packet) = Packet::parse(buf.get(..len).unwrap_or_default()) else {
            continue;
        };
        let request = Request {
            peer,
            packet,
            started: Instant::now(),
        };
        let task_source = Arc::clone(&source);
        let task_admission = Arc::clone(&admission);
        let task_observer = Arc::clone(&observer);
        drop(tokio::spawn(async move {
            let path_hint = match &request.packet {
                Packet::Read { filename, .. } => filename.clone(),
                Packet::Write
                | Packet::Data { .. }
                | Packet::Ack { .. }
                | Packet::Error { .. }
                | Packet::OptionAck { .. } => String::new(),
            };
            let started = request.started;
            match handle(
                request,
                task_source,
                config,
                task_admission,
                Arc::clone(&task_observer),
            )
            .await
            {
                Ok(outcome) => task_observer.finished(&outcome),
                Err(error) => {
                    tracing::warn!(%peer, %error, "tftp transfer failed");
                    task_observer.finished(&Outcome {
                        peer,
                        path: path_hint,
                        completed: false,
                        blksize: 0,
                        window: 0,
                        bytes: 0,
                        elapsed: started.elapsed(),
                    });
                }
            }
        }));
    }
}

#[cfg(test)]
mod tests;
