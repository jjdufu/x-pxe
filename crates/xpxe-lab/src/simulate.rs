//! `simulate-install`：不启动虚拟机，按真实目标机的请求顺序走完一次装机（第 12 步联调）。
//!
//! `xpxe-lab simulate-install --boot-base http://127.0.0.1:8080 --mac <mac>
//!  [--step-delay-ms 1500] [--module-delay-ms 100]`
//!
//! 顺序与 d-i/agent 一致：`/boot/mac` → `boot.ipxe` → `preseed.cfg` → `installer.started` →
//! `late.sh` → `plan.json`（agent 令牌）→ `installer.finished` →（“重启”）→ `agent.started` →
//! 每个启用模块的 `module.started/finished` → `agent.finished`。
//! 每一步输出一行 JSON；令牌只用于请求路径，从不输出。

use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::common::{Args, LabError, LabResult, emit, format_mac, parse_mac, runtime};

/// 引导端地址：`host:port` 与路径前缀。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootBase {
    /// `host:port`。
    host_port: String,
    /// 路径前缀（无结尾斜杠）。
    prefix: String,
}

/// 一次 HTTP 响应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Response {
    /// 状态码。
    status: u16,
    /// 响应体文本。
    body: String,
}

/// 解析 `http://host:port[/prefix]`（只支持明文 HTTP，引导端本就是明文）。
///
/// # Errors
///
/// 不是 `http://` 地址时返回错误。
pub(crate) fn parse_base(raw: &str) -> LabResult<BootBase> {
    let rest = raw
        .strip_prefix("http://")
        .ok_or_else(|| LabError(format!("--boot-base 必须是 http:// 地址：{raw}")))?;
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host_port.is_empty() {
        return Err(LabError(format!("--boot-base 缺少主机：{raw}")));
    }
    let trimmed = path.trim_end_matches('/');
    Ok(BootBase {
        host_port: host_port.to_owned(),
        prefix: if trimmed.is_empty() {
            String::new()
        } else {
            format!("/{trimmed}")
        },
    })
}

/// 解码 chunked 响应体。
fn dechunk(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some((size_line, tail)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size_line.trim(), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        out.push_str(tail.get(..size).unwrap_or(tail));
        rest = tail.get(size..).unwrap_or("").trim_start_matches("\r\n");
    }
    out
}

/// 解析原始 HTTP 响应。
///
/// # Errors
///
/// 格式不符时返回错误。
pub(crate) fn parse_response(raw: &[u8]) -> LabResult<Response> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| LabError("HTTP 响应缺少头部结束标记".to_owned()))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| LabError("HTTP 状态行非法".to_owned()))?;
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    Ok(Response {
        status,
        body: if chunked {
            dechunk(body)
        } else {
            body.to_owned()
        },
    })
}

/// 发送一次请求（`Connection: close`）。
///
/// # Errors
///
/// 连接或读写失败时返回错误。
async fn request(base: &BootBase, method: &str, path: &str, body: &str) -> LabResult<Response> {
    let mut stream = tokio::net::TcpStream::connect(base.host_port.as_str()).await?;
    let content_type = if body.starts_with('{') {
        "application/json"
    } else {
        "text/plain"
    };
    let head = format!(
        "{method} {}{path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: xpxe-lab-simulate\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        base.prefix,
        base.host_port,
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.flush().await?;
    let mut raw = Vec::new();
    let _read = stream.read_to_end(&mut raw).await?;
    parse_response(&raw)
}

/// 期望 2xx。
///
/// # Errors
///
/// 非 2xx 时返回错误（只带路径的种类，不带令牌）。
async fn expect_ok(
    base: &BootBase,
    method: &str,
    path: &str,
    body: &str,
    step: &str,
) -> LabResult<String> {
    let reply = request(base, method, path, body).await?;
    if (200..300).contains(&reply.status) {
        emit(&json!({ "event": "step", "step": step, "status": reply.status }))?;
        Ok(reply.body)
    } else {
        Err(LabError(format!(
            "{step} → HTTP {}：{}",
            reply.status,
            reply.body.trim()
        )))
    }
}

/// 取 `/p/{token}/{file}` 中 `file` 之前的令牌（跳过同名的其他路径）。
pub(crate) fn token_before(text: &str, file: &str) -> Option<String> {
    let needle = format!("/{file}");
    text.match_indices(&needle).find_map(|(end, _)| {
        let head = text.get(..end)?;
        let start = head.rfind("/p/")?.checked_add(3)?;
        let token = head.get(start..)?;
        (!token.is_empty() && !token.contains(['/', ' ', '\'', '"'])).then(|| token.to_owned())
    })
}

/// 从 preseed 中取会话 ID（`event_id=<session>.installer`）。
pub(crate) fn session_of(preseed: &str) -> Option<String> {
    let start = preseed.find("event_id=")?.checked_add("event_id=".len())?;
    let rest = preseed.get(start..)?;
    let end = rest.find(".installer")?;
    rest.get(..end).map(str::to_owned)
}

/// 计划中启用的模块 ID。
pub(crate) fn enabled_modules(plan: &Value) -> Vec<String> {
    plan.get("modules")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |list| {
            list.iter()
                .filter(|entry| entry.get("enabled").and_then(Value::as_bool) != Some(false))
                .filter_map(|entry| entry.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
}

/// 节奏参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pace {
    /// 阶段之间的间隔。
    step: Duration,
    /// 模块之间的间隔。
    module: Duration,
}

/// 安装器阶段取得的会话信息。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tokens {
    /// 会话 ID。
    session: String,
    /// Boot 令牌。
    boot: String,
    /// Agent 令牌。
    agent: String,
}

/// 安装器阶段（到 `late.sh` 为止）。
///
/// # Errors
///
/// 任一步失败时返回错误。
async fn installer(base: &BootBase, mac: &str, pace: Pace) -> LabResult<Tokens> {
    let script = expect_ok(
        base,
        "GET",
        &format!("/boot/mac/{}", mac.replace(':', "-")),
        "",
        "boot.mac",
    )
    .await?;
    let boot = token_before(&script, "boot.ipxe")
        .ok_or_else(|| LabError(format!("节点 {mac} 未 arm：/boot/mac 未返回会话脚本")))?;
    let _ipxe = expect_ok(
        base,
        "GET",
        &format!("/p/{boot}/boot.ipxe"),
        "",
        "boot.ipxe",
    )
    .await?;
    let preseed = expect_ok(
        base,
        "GET",
        &format!("/p/{boot}/preseed.cfg"),
        "",
        "preseed.cfg",
    )
    .await?;
    let session =
        session_of(&preseed).ok_or_else(|| LabError("preseed 中没有会话 ID".to_owned()))?;
    let started = format!("event=installer.started event_id={session}.installer");
    let _started = expect_ok(
        base,
        "POST",
        &format!("/p/{boot}/events/installer.started"),
        &started,
        "installer.started",
    )
    .await?;
    tokio::time::sleep(pace.step).await;
    let late = expect_ok(base, "GET", &format!("/p/{boot}/late.sh"), "", "late.sh").await?;
    let agent = token_before(&late, "plan.json")
        .ok_or_else(|| LabError("late.sh 中没有计划地址".to_owned()))?;
    Ok(Tokens {
        session,
        boot,
        agent,
    })
}

/// agent 阶段。
///
/// # Errors
///
/// 任一步失败时返回错误。
async fn agent(
    base: &BootBase,
    session: &str,
    token: &str,
    modules: &[String],
    pace: Pace,
) -> LabResult {
    let path = format!("/p/{token}/events");
    let post = |kind: &'static str, module: Option<&str>, payload: Value| {
        let event_id = module.map_or_else(
            || format!("{session}.{kind}"),
            |id| format!("{session}.{kind}.{id}"),
        );
        let body = json!({ "event_id": event_id, "kind": kind, "payload": payload }).to_string();
        let step = module.map_or_else(|| kind.to_owned(), |id| format!("{kind}:{id}"));
        let target = path.clone();
        async move { expect_ok(base, "POST", &target, &body, &step).await }
    };
    let _started = post("agent.started", None, json!({})).await?;
    for module in modules {
        let _begin = post("module.started", Some(module), json!({ "title": module })).await?;
        tokio::time::sleep(pace.module).await;
        let done =
            json!({ "status": "ok", "exit_code": 0_i32, "timed_out": false, "elapsed_s": 0_u64 });
        let _end = post("module.finished", Some(module), done).await?;
    }
    tokio::time::sleep(pace.step).await;
    let _finished = post(
        "agent.finished",
        None,
        json!({ "modules": modules.len(), "reboot": true }),
    )
    .await?;
    Ok(())
}

/// 走完整流程。
///
/// # Errors
///
/// 任一步失败时返回错误。
async fn simulate(base: &BootBase, mac: &str, pace: Pace) -> LabResult {
    let tokens = installer(base, mac, pace).await?;
    let plan_text = expect_ok(
        base,
        "GET",
        &format!("/p/{}/plan.json", tokens.agent),
        "",
        "plan.json",
    )
    .await?;
    let plan: Value = serde_json::from_str(&plan_text)?;
    let modules = enabled_modules(&plan);
    let finished = format!(
        "event=installer.finished event_id={}.installer",
        tokens.session
    );
    let _finished = expect_ok(
        base,
        "POST",
        &format!("/p/{}/events/installer.finished", tokens.boot),
        &finished,
        "installer.finished",
    )
    .await?;
    tokio::time::sleep(pace.step).await;
    agent(base, &tokens.session, &tokens.agent, &modules, pace).await?;
    emit(&json!({ "event": "done", "session": tokens.session, "modules": modules.len() }))
}

/// 子命令入口。
///
/// # Errors
///
/// 参数非法或任一步失败时返回错误。
pub(crate) fn run(args: &Args) -> LabResult {
    let base = parse_base(&args.required("--boot-base")?)?;
    let mac = format_mac(parse_mac(&args.required("--mac")?)?);
    let pace = Pace {
        step: Duration::from_millis(args.parsed("--step-delay-ms", 1500_u64)?),
        module: Duration::from_millis(args.parsed("--module-delay-ms", 100_u64)?),
    };
    let outcome = runtime()?.block_on(simulate(&base, &mac, pace));
    if let Err(error) = &outcome {
        emit(&json!({ "event": "error", "message": error.0 }))?;
    }
    outcome
}

#[cfg(test)]
mod tests;
