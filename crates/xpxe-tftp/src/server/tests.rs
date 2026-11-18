//! 传输层回环测试。

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use xpxe_test_support::{TestFailure, TestResult, ensure, ensure_eq};

use super::{Observer, Outcome, ServeConfig, Source, normalize, serve};
use crate::packet::{ErrorCode, Packet};

/// 记录全部结束事件的观测器。
#[derive(Default)]
struct Recorder(std::sync::Mutex<Vec<Outcome>>);

impl Observer for Recorder {
    fn started(&self, _peer: SocketAddr, _path: &str) {}

    fn finished(&self, outcome: &Outcome) {
        if let Ok(mut list) = self.0.lock() {
            list.push(outcome.clone());
        }
    }
}

impl Recorder {
    /// 已完成传输的数量。
    fn completed(&self) -> usize {
        self.0
            .lock()
            .map_or(0, |list| list.iter().filter(|item| item.completed).count())
    }
}

/// 内存文件来源。
struct Memory(BTreeMap<String, Arc<[u8]>>);

impl Source for Memory {
    fn open(&self, path: &str) -> Option<Arc<[u8]>> {
        self.0.get(path).cloned()
    }
}

/// 路径规范化。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn normalizes_paths() -> TestResult {
    ensure_eq(
        &normalize("/boot\\ipxe.efi"),
        &Some("boot/ipxe.efi".to_owned()),
        "分隔符",
    )?;
    ensure_eq(&normalize("./a//b"), &Some("a/b".to_owned()), "冗余")?;
    ensure_eq(&normalize("../etc/passwd"), &None, "上级目录")?;
    ensure_eq(&normalize("/"), &None, "空路径")?;
    Ok(())
}

/// 接收一个报文（带超时）。
///
/// # Errors
///
/// 超时、套接字或解析错误。
async fn recv(socket: &UdpSocket) -> TestResult<(Packet, SocketAddr)> {
    let mut buf = vec![0_u8; 65536];
    let (len, from) = tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf))
        .await
        .map_err(|elapsed| TestFailure::new(format!("接收超时: {elapsed}")))??;
    Ok((Packet::parse(buf.get(..len).unwrap_or_default())?, from))
}

/// 客户端流程：带 blksize/tsize 下载整个文件，并验证不存在文件与写请求的错误。
///
/// # Errors
///
/// 下载内容或错误码不符时返回错误。
async fn client_flow() -> TestResult {
    let content: Vec<u8> = (0..5000_u32)
        .map(|value| u8::try_from(value % 251).unwrap_or(0))
        .collect();
    let mut files = BTreeMap::new();
    let _old = files.insert(
        "boot/ipxe.efi".to_owned(),
        Arc::<[u8]>::from(content.clone()),
    );
    let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = server.local_addr()?;
    let recorder = Arc::new(Recorder::default());
    let shared = Arc::clone(&recorder);
    let observer: Arc<dyn Observer> = shared;
    let task = tokio::spawn(serve(
        server,
        Arc::new(Memory(files)),
        ServeConfig::new(Ipv4Addr::LOCALHOST.into()),
        observer,
    ));

    let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    download(&client, addr, &content).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    ensure_eq(&recorder.completed(), &1, "观测到一次完成")?;

    refusals(&client, addr).await?;
    task.abort();
    Ok(())
}

/// 带 blksize/tsize 下载 `boot/ipxe.efi` 并核对内容。
///
/// # Errors
///
/// 协商或内容不符时返回错误。
async fn download(client: &UdpSocket, addr: SocketAddr, content: &[u8]) -> TestResult {
    let request = Packet::Read {
        filename: "/boot/ipxe.efi".to_owned(),
        mode: "octet".to_owned(),
        options: vec![
            ("blksize".to_owned(), "1024".to_owned()),
            ("tsize".to_owned(), "0".to_owned()),
        ],
    };
    let _sent = client.send_to(&request.encode(), addr).await?;
    let (oack, peer) = recv(client).await?;
    ensure(peer != addr, "传输应使用临时端口")?;
    ensure_eq(
        &oack,
        &Packet::OptionAck {
            options: vec![
                ("blksize".to_owned(), "1024".to_owned()),
                ("tsize".to_owned(), "5000".to_owned()),
            ],
        },
        "OACK",
    )?;
    let _ack0 = client
        .send_to(&Packet::Ack { block: 0 }.encode(), peer)
        .await?;
    let mut received = Vec::new();
    loop {
        let (packet, _from) = recv(client).await?;
        let Packet::Data { block, data } = packet else {
            return Err(Box::new(TestFailure::new("期望数据块")));
        };
        let last = data.len() < 1024;
        received.extend_from_slice(&data);
        let _ack = client
            .send_to(&Packet::Ack { block }.encode(), peer)
            .await?;
        if last {
            break;
        }
    }
    ensure_eq(&received.as_slice(), &content, "内容")?;
    Ok(())
}

/// 不存在的文件与写请求被拒绝。
///
/// # Errors
///
/// 错误码不符时返回错误。
async fn refusals(client: &UdpSocket, addr: SocketAddr) -> TestResult {
    let missing = Packet::Read {
        filename: "nope".to_owned(),
        mode: "octet".to_owned(),
        options: Vec::new(),
    };
    let _again = client.send_to(&missing.encode(), addr).await?;
    let (error, _peer) = recv(client).await?;
    ensure(
        matches!(
            error,
            Packet::Error {
                code: ErrorCode::FileNotFound,
                ..
            }
        ),
        "文件不存在",
    )?;
    let _write = client.send_to(&Packet::Write.encode(), addr).await?;
    let (denied, _peer2) = recv(client).await?;
    ensure(
        matches!(
            denied,
            Packet::Error {
                code: ErrorCode::AccessViolation,
                ..
            }
        ),
        "写请求被拒绝",
    )?;
    let netascii = Packet::Read {
        filename: "boot/ipxe.efi".to_owned(),
        mode: "netascii".to_owned(),
        options: Vec::new(),
    };
    let _ascii = client.send_to(&netascii.encode(), addr).await?;
    let (ascii, _peer3) = recv(client).await?;
    ensure(
        matches!(
            ascii,
            Packet::Error {
                code: ErrorCode::IllegalOperation,
                ..
            }
        ),
        "netascii 被拒绝",
    )?;
    Ok(())
}

/// 每源 IP 并发上限：第 3 个并发读请求（上限 2）得到 busy 错误。
///
/// # Errors
///
/// 行为不符时返回错误。
async fn busy_flow() -> TestResult {
    let mut files = BTreeMap::new();
    let _old = files.insert("f".to_owned(), Arc::<[u8]>::from(vec![1_u8; 4096]));
    let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = server.local_addr()?;
    let mut config = ServeConfig::new(Ipv4Addr::LOCALHOST.into());
    config.max_per_client = 2;
    let task = tokio::spawn(serve(
        server,
        Arc::new(Memory(files)),
        config,
        Arc::new(Recorder::default()),
    ));
    let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let read = Packet::Read {
        filename: "f".to_owned(),
        mode: "octet".to_owned(),
        options: Vec::new(),
    };
    for _ in 0_u8..2 {
        let _sent = client.send_to(&read.encode(), addr).await?;
        let (first, _peer) = recv(&client).await?;
        ensure(
            matches!(first, Packet::Data { block: 1, .. }),
            "前两个传输开始",
        )?;
    }
    let _third = client.send_to(&read.encode(), addr).await?;
    let (busy, _peer) = recv(&client).await?;
    ensure(
        matches!(
            busy,
            Packet::Error {
                code: ErrorCode::Undefined,
                ..
            }
        ),
        "超过每源上限回 busy",
    )?;
    task.abort();
    Ok(())
}

/// 并发上限。
///
/// # Errors
///
/// 流程失败时返回错误。
#[test]
fn per_client_limit() -> TestResult {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(busy_flow())?;
    Ok(())
}

/// 在多线程运行时中执行客户端流程。
///
/// # Errors
///
/// 流程失败时返回错误。
#[test]
fn loopback_download() -> TestResult {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(client_flow())?;
    Ok(())
}
