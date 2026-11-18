//! 事件回报：JSON `POST` 到计划中的 `events_url`，带 `event_id` 与指数退避重试。

use std::io::{Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::host::Host;

/// 一条事件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentEvent {
    /// 幂等 ID（会话 + 种类 + 模块，重发时不变）。
    pub event_id: String,
    /// 种类：`agent.started|finished|failed`、`module.started|finished`。
    pub kind: String,
    /// 附加数据。
    pub payload: Value,
}

impl AgentEvent {
    /// 构造事件；`module` 为空时 ID 不含模块段。
    #[must_use]
    pub fn new(session: &str, kind: &str, module: Option<&str>, payload: Value) -> Self {
        let event_id = module.map_or_else(
            || format!("{session}.{kind}"),
            |id| format!("{session}.{kind}.{id}"),
        );
        Self {
            event_id,
            kind: kind.to_owned(),
            payload,
        }
    }

    /// 是否为最终事件。
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.kind == "agent.finished" || self.kind == "agent.failed"
    }
}

/// 回报通道。
pub trait Reporter {
    /// 发送一次。
    ///
    /// # Errors
    ///
    /// 网络错误或非 2xx 时返回说明。
    fn send(&mut self, event: &AgentEvent) -> Result<(), String>;
}

/// 重试策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// 普通事件最多尝试次数。
    pub progress_attempts: u32,
    /// 最终事件持续重试时长。
    pub final_window: Duration,
    /// 首次退避。
    pub base_delay: Duration,
    /// 退避上限。
    pub max_delay: Duration,
}

impl RetryPolicy {
    /// 计划给出的最终重试窗口与默认退避。
    #[must_use]
    pub const fn with_final_window(final_window: Duration) -> Self {
        Self {
            progress_attempts: 5,
            final_window,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

/// 按策略投递：普通事件有限次重试，最终事件在窗口内持续重试。
///
/// # Errors
///
/// 放弃时返回最后一次错误说明。
pub fn deliver<H: Host, R: Reporter>(
    host: &mut H,
    reporter: &mut R,
    policy: RetryPolicy,
    event: &AgentEvent,
) -> Result<(), String> {
    let start = host.now();
    let mut delay = policy.base_delay;
    let mut attempts: u32 = 0;
    loop {
        let error = match reporter.send(event) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        attempts = attempts.saturating_add(1);
        let elapsed = host.now().saturating_duration_since(start);
        let give_up = if event.is_final() {
            elapsed.saturating_add(delay) > policy.final_window
        } else {
            attempts >= policy.progress_attempts
        };
        if give_up {
            return Err(format!("{} 未送达（{attempts} 次）：{error}", event.kind));
        }
        host.sleep(delay);
        delay = delay.saturating_mul(2).min(policy.max_delay);
    }
}

/// 基于 `std::net` 的最小 HTTP/1.1 客户端（只支持 `http://`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReporter {
    /// `host[:port]`。
    authority: String,
    /// 路径。
    path: String,
    /// 连接与读写超时。
    timeout: Duration,
}

impl HttpReporter {
    /// 解析事件地址。
    ///
    /// # Errors
    ///
    /// 不是 `http://host[:port]/path` 时返回说明。
    pub fn new(url: &str) -> Result<Self, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| format!("只支持 http://：{url}"))?;
        let (authority, path) = rest.split_once('/').map_or((rest, "/"), |(host, _)| {
            (host, rest.get(host.len()..).unwrap_or("/"))
        });
        if authority.is_empty() {
            return Err(format!("缺少主机：{url}"));
        }
        Ok(Self {
            authority: authority.to_owned(),
            path: path.to_owned(),
            timeout: Duration::from_secs(10),
        })
    }

    /// 带默认端口 80 的连接地址。
    fn address(&self) -> String {
        if self.authority.contains(':') {
            self.authority.clone()
        } else {
            format!("{}:80", self.authority)
        }
    }
}

/// 读到第一行（状态行）为止，不等对端关闭连接。
///
/// # Errors
///
/// 读失败或对端提前关闭时返回说明。
fn read_status_line(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut reply = Vec::new();
    let mut chunk = [0_u8; 256];
    while !reply.contains(&b'\n') && reply.len() < 4096 {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        reply.extend(chunk.iter().take(count));
    }
    if reply.is_empty() {
        return Err("对端未回应".to_owned());
    }
    Ok(reply)
}

impl Reporter for HttpReporter {
    fn send(&mut self, event: &AgentEvent) -> Result<(), String> {
        let body = serde_json::to_vec(event).map_err(|error| error.to_string())?;
        let addr = self
            .address()
            .to_socket_addrs()
            .map_err(|error| format!("解析 {}：{error}", self.authority))?
            .next()
            .ok_or_else(|| format!("{} 没有地址", self.authority))?;
        let mut stream =
            TcpStream::connect_timeout(&addr, self.timeout).map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|error| error.to_string())?;
        let head = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nUser-Agent: xpxe-agent\r\n\r\n",
            self.path,
            self.authority,
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .map_err(|error| error.to_string())?;
        stream.write_all(&body).map_err(|error| error.to_string())?;
        let reply = read_status_line(&mut stream)?;
        let status_line = String::from_utf8_lossy(&reply);
        let code = status_line.split_whitespace().nth(1).unwrap_or("");
        if code.starts_with('2') {
            Ok(())
        } else {
            Err(format!("HTTP 状态 {code:?}"))
        }
    }
}
