//! 三个监听器（管理 / 引导 / 内部）与后台 worker；自写 accept 循环以注入对端地址，管理端可选 TLS。
//! 收到停止信号后停止接受新连接，在途连接优雅收尾，最多等待 10 秒。

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use http::Request;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt as _;

use crate::app::App;
use crate::auth::Peer;
use crate::{admin, boot, effects, internal, timers, tls};

/// 优雅退出的最长等待。
const DRAIN: Duration = Duration::from_secs(10);
/// 请求头读取超时（防慢速攻击）。
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);

/// 运行中的控制面。
#[derive(Debug)]
pub struct Running {
    /// 应用状态。
    app: Arc<App>,
    /// 管理端实际地址。
    admin: SocketAddr,
    /// 引导端实际地址。
    boot: SocketAddr,
    /// 内部 API 实际地址。
    internal: SocketAddr,
    /// 停止信号。
    stop: watch::Sender<bool>,
    /// 监听与 worker 任务。
    tasks: JoinSet<()>,
}

impl Running {
    /// 管理端地址。
    #[must_use]
    pub const fn admin_addr(&self) -> SocketAddr {
        self.admin
    }

    /// 引导端地址。
    #[must_use]
    pub const fn boot_addr(&self) -> SocketAddr {
        self.boot
    }

    /// 内部 API 地址。
    #[must_use]
    pub const fn internal_addr(&self) -> SocketAddr {
        self.internal
    }

    /// 应用状态。
    #[must_use]
    pub fn app(&self) -> Arc<App> {
        Arc::clone(&self.app)
    }

    /// 停止并等待收尾（最多 10 秒，超时强制中止）。
    pub async fn shutdown(mut self) {
        let _sent = self.stop.send(true);
        let drained = tokio::time::timeout(DRAIN, async {
            while self.tasks.join_next().await.is_some() {}
        })
        .await;
        if drained.is_err() {
            tracing::warn!("优雅退出超时，强制中止剩余连接");
            self.tasks.abort_all();
        }
    }
}

/// 服务一条连接：注入对端地址，停止信号到达时优雅关闭。
async fn serve_connection<S>(
    stream: S,
    peer: SocketAddr,
    router: Router,
    mut stop: watch::Receiver<bool>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = router.map_request(move |mut request: Request<Incoming>| {
        let _previous = request.extensions_mut().insert(Peer(peer));
        request
    });
    let mut builder = http1::Builder::new();
    let _configured = builder
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT);
    let connection =
        builder.serve_connection(TokioIo::new(stream), TowerToHyperService::new(service));
    tokio::pin!(connection);
    tokio::select! {
        result = connection.as_mut() => {
            if let Err(error) = result {
                tracing::debug!(%error, %peer, "连接结束");
            }
        }
        _changed = stop.changed() => {
            connection.as_mut().graceful_shutdown();
            if let Err(error) = connection.await {
                tracing::debug!(%error, %peer, "收尾时连接出错");
            }
        }
    }
}

/// 接受循环。
async fn accept_loop(
    listener: TcpListener,
    router: Router,
    tls: Option<TlsAcceptor>,
    mut stop: watch::Receiver<bool>,
) {
    let mut connections = JoinSet::new();
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            _changed = stop.changed() => break,
        };
        let (stream, peer) = match accepted {
            Ok(pair) => pair,
            Err(error) => {
                tracing::warn!(%error, "accept 失败");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let service = router.clone();
        let watcher = stop.clone();
        let secure = tls.clone();
        let _handle = connections.spawn(async move {
            match secure {
                None => serve_connection(stream, peer, service, watcher).await,
                Some(acceptor) => {
                    match tokio::time::timeout(HEADER_TIMEOUT, acceptor.accept(stream)).await {
                        Ok(Ok(secured)) => serve_connection(secured, peer, service, watcher).await,
                        Ok(Err(error)) => tracing::debug!(%error, %peer, "TLS 握手失败"),
                        Err(_elapsed) => tracing::debug!(%peer, "TLS 握手超时"),
                    }
                }
            }
        });
        while connections.try_join_next().is_some() {}
    }
    while connections.join_next().await.is_some() {}
}

/// 绑定监听。
///
/// # Errors
///
/// 端口不可用时返回错误。
async fn bind(addr: SocketAddr, what: &str) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
        .await
        .map_err(|error| io::Error::new(error.kind(), format!("{what} 监听 {addr} 失败：{error}")))
}

/// 启动三个监听器与后台 worker。
///
/// # Errors
///
/// TLS 证书不可用或端口绑定失败时返回错误。
pub async fn start(app: Arc<App>) -> io::Result<Running> {
    let acceptor = match &app.settings.tls {
        Some((cert, key)) => Some(TlsAcceptor::from(
            tls::server_config(cert, key).map_err(io::Error::other)?,
        )),
        None => None,
    };
    let admin_listener = bind(app.settings.admin_listen, "管理端").await?;
    let boot_listener = bind(app.settings.boot_listen, "引导端").await?;
    let internal_listener = bind(app.settings.internal_listen, "内部 API").await?;
    let (stop, watcher) = watch::channel(false);
    let mut running = Running {
        admin: admin_listener.local_addr()?,
        boot: boot_listener.local_addr()?,
        internal: internal_listener.local_addr()?,
        app: Arc::clone(&app),
        stop,
        tasks: JoinSet::new(),
    };
    let tasks = &mut running.tasks;
    let _admin = tasks.spawn(accept_loop(
        admin_listener,
        admin::router(Arc::clone(&app)),
        acceptor,
        watcher.clone(),
    ));
    let _boot = tasks.spawn(accept_loop(
        boot_listener,
        boot::router(Arc::clone(&app)),
        None,
        watcher.clone(),
    ));
    let _internal = tasks.spawn(accept_loop(
        internal_listener,
        internal::router(Arc::clone(&app)),
        None,
        watcher.clone(),
    ));
    let _effects = tasks.spawn(effects::run(Arc::clone(&app), watcher.clone()));
    let _manual = tasks.spawn(crate::manual::prepare(Arc::clone(&app)));
    let _timers = tasks.spawn(timers::run(app, watcher));
    Ok(running)
}
