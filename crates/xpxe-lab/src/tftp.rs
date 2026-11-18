//! `tftp`：最小只读 TFTP，只服务命令行列出的文件。

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::json;
use xpxe_tftp::server::{ServeConfig, Source, TracingObserver, serve};

use crate::common::{Args, LabError, LabResult, emit};

/// 启动时载入内存的文件表。
#[derive(Debug)]
struct Files(BTreeMap<String, Arc<[u8]>>);

impl Source for Files {
    fn open(&self, path: &str) -> Option<Arc<[u8]>> {
        self.0.get(path).cloned()
    }
}

/// 子命令入口：`--root-file <name>=<path>`（可重复）、`--listen <ip:port>`（默认 10.77.0.1:69）。
///
/// # Errors
///
/// 参数非法、文件不可读或套接字失败时返回 [`LabError`]。
pub(crate) fn run(args: &Args) -> LabResult {
    let mut files = BTreeMap::new();
    for item in args.values("--root-file") {
        let (name, path) = item
            .split_once('=')
            .ok_or_else(|| LabError(format!("--root-file 需为 <name>=<path>：{item}")))?;
        let data = std::fs::read(path)?;
        emit(&json!({"event": "file", "name": name, "path": path, "bytes": data.len()}))?;
        let _previous = files.insert(name.to_owned(), Arc::<[u8]>::from(data));
    }
    if files.is_empty() {
        return Err(LabError("至少需要一个 --root-file".to_owned()));
    }
    let listen: SocketAddr = args.parsed("--listen", SocketAddr::from(([10, 77, 0, 1], 69)))?;
    let runtime = crate::common::runtime()?;
    runtime.block_on(async move {
        let socket = tokio::net::UdpSocket::bind(listen).await?;
        emit(&json!({"event": "listening", "addr": listen.to_string()}))?;
        crate::logging::init();
        serve(
            socket,
            Arc::new(Files(files)),
            ServeConfig::new(listen.ip()),
            Arc::new(TracingObserver),
        )
        .await?;
        Ok(())
    })
}
