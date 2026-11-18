//! 编排用例。

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use xpxe_agent::{AgentEvent, HttpReporter, Reporter as _, RunOutcome, run};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use crate::fake::{FakeHost, Recorder, Workspace, disabled, entry, exists};

/// 成功路径：事件顺序、日志前缀、跳过记录、清理与重启。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn success_reports_in_order_and_cleans_up() -> TestResult {
    let ws = Workspace::new(
        vec![
            entry("alpha", "echo hello-alpha\n", 30),
            disabled("beta"),
            entry("gamma", "echo err >&2\n", 30),
        ],
        true,
    )?;
    let mut host = FakeHost::root();
    let mut reporter = Recorder::default();
    let outcome = run(&ws.config, &mut host, &mut reporter)?;
    ensure_eq(
        &outcome,
        &RunOutcome::Finished { rebooted: true },
        "成功并重启",
    )?;
    let expected = [
        "sess01.agent.started",
        "sess01.module.started.alpha",
        "sess01.module.finished.alpha",
        "sess01.module.started.gamma",
        "sess01.module.finished.gamma",
        "sess01.agent.finished",
    ];
    ensure_eq(
        &reporter.trail(),
        &expected.map(str::to_owned).to_vec(),
        "事件顺序",
    )?;
    ensure_eq(
        &host.disabled,
        &vec!["xpxe-firstboot.service".to_owned()],
        "禁用服务",
    )?;
    ensure_eq(&host.reboots, &1, "重启一次")?;
    ensure(!exists(&ws.config.plan_path), "计划已删除")?;
    ensure(!exists(&ws.path("xpxe-agent")), "agent 自身已删除")?;
    let log = ws.log()?;
    ensure(log.contains("[alpha] hello-alpha"), "stdout 带前缀写入日志")?;
    ensure(log.contains("[gamma] err"), "stderr 带前缀写入日志")?;
    ensure(log.contains("beta 跳过（未启用）"), "跳过的模块有记录")?;
    Ok(())
}

/// 模块失败：保留计划、不重启、不跑后续模块、回报日志尾。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn failure_keeps_plan_and_does_not_reboot() -> TestResult {
    let scratch = Workspace::new(vec![], true)?;
    let after = scratch.path("after-ran");
    let ws = Workspace::new(
        vec![
            entry("alpha", "echo one\n", 30),
            entry(
                "beta",
                "for n in 1 2 3; do echo line-$n; done\nexit 7\n",
                30,
            ),
            entry("gamma", &format!("touch {}\n", after.display()), 30),
        ],
        true,
    )?;
    let mut host = FakeHost::root();
    let mut reporter = Recorder::default();
    let outcome = run(&ws.config, &mut host, &mut reporter)?;
    ensure_eq(
        &outcome,
        &RunOutcome::Failed {
            module: "beta".to_owned(),
        },
        "失败于 beta",
    )?;
    ensure_eq(&host.reboots, &0, "失败不重启")?;
    ensure_eq(&host.disabled.len(), &1, "失败也禁用服务")?;
    ensure(exists(&ws.config.plan_path), "计划保留")?;
    ensure(exists(&ws.path("xpxe-agent")), "agent 保留")?;
    ensure(!exists(&after), "失败后不再执行后续模块")?;
    let last = reporter.events.last().ok_or("没有事件")?;
    ensure_eq(&last.kind.as_str(), &"agent.failed", "最终事件")?;
    ensure_eq(
        &last
            .payload
            .get("module")
            .and_then(serde_json::Value::as_str),
        &Some("beta"),
        "失败模块",
    )?;
    ensure_eq(
        &last
            .payload
            .get("exit_code")
            .and_then(serde_json::Value::as_i64),
        &Some(7),
        "退出码",
    )?;
    let tail = last
        .payload
        .get("log_tail")
        .and_then(serde_json::Value::as_array)
        .ok_or("缺 log_tail")?;
    ensure_eq(&tail.len(), &3, "日志尾")?;
    Ok(())
}

/// 超时：杀死整个进程组并很快返回。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn timeout_kills_process_group() -> TestResult {
    let ws = Workspace::new(vec![entry("slow", "sleep 30 &\nsleep 30\n", 1)], false)?;
    let mut host = FakeHost::root();
    let mut reporter = Recorder::default();
    let start = Instant::now();
    let outcome = run(&ws.config, &mut host, &mut reporter)?;
    ensure(
        start.elapsed() < Duration::from_secs(10),
        "超时后应很快返回",
    )?;
    ensure_eq(
        &outcome,
        &RunOutcome::Failed {
            module: "slow".to_owned(),
        },
        "超时视为失败",
    )?;
    let last = reporter.events.last().ok_or("没有事件")?;
    ensure_eq(
        &last
            .payload
            .get("timed_out")
            .and_then(serde_json::Value::as_bool),
        &Some(true),
        "timed_out",
    )?;
    Ok(())
}

/// 续跑：已完成模块按标记跳过。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn resume_skips_completed_modules() -> TestResult {
    let scratch = Workspace::new(vec![], false)?;
    let count = scratch.path("count");
    let gate = scratch.path("gate");
    let ws = Workspace::new(
        vec![
            entry("alpha", &format!("echo x >> {}\n", count.display()), 30),
            entry("beta", &format!("test -f {}\n", gate.display()), 30),
        ],
        false,
    )?;
    let mut host = FakeHost::root();
    let mut first = Recorder::default();
    ensure_eq(
        &run(&ws.config, &mut host, &mut first)?,
        &RunOutcome::Failed {
            module: "beta".to_owned(),
        },
        "首次失败",
    )?;
    fs::write(&gate, b"")?;
    let mut second = Recorder::default();
    ensure_eq(
        &run(&ws.config, &mut host, &mut second)?,
        &RunOutcome::Finished { rebooted: false },
        "续跑成功",
    )?;
    ensure_eq(
        &fs::read_to_string(&count)?,
        &"x\n".to_owned(),
        "alpha 只执行一次",
    )?;
    ensure(
        !second
            .trail()
            .contains(&"sess01.module.started.alpha".to_owned()),
        "续跑不再上报 alpha",
    )?;
    ensure(
        ws.log()?.contains("alpha 跳过（已完成，续跑）"),
        "续跑跳过有记录",
    )?;
    Ok(())
}

/// `continue_on_error` 失败后继续。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn continue_on_error_proceeds() -> TestResult {
    let mut soft = entry("soft", "exit 3\n", 30);
    soft.continue_on_error = true;
    let ws = Workspace::new(vec![soft, entry("next", "true\n", 30)], false)?;
    let mut host = FakeHost::root();
    let mut reporter = Recorder::default();
    ensure_eq(
        &run(&ws.config, &mut host, &mut reporter)?,
        &RunOutcome::Finished { rebooted: false },
        "继续完成",
    )?;
    ensure(
        reporter
            .trail()
            .contains(&"sess01.module.finished.next".to_owned()),
        "后续模块执行",
    )?;
    Ok(())
}

/// 非 root 与摘要不符时拒绝执行。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn refuses_non_root_and_bad_digest() -> TestResult {
    let ws = Workspace::new(vec![entry("alpha", "true\n", 30)], false)?;
    let mut host = FakeHost::root();
    host.root = false;
    let mut reporter = Recorder::default();
    let error = run(&ws.config, &mut host, &mut reporter)
        .err()
        .ok_or("非 root 应失败")?;
    ensure_eq(&error.code, &"not_root", "错误码")?;
    ensure(reporter.events.is_empty(), "非 root 不回报")?;
    let mut bad = entry("alpha", "true\n", 30);
    bad.script = "rm -rf /\n".to_owned();
    let tampered = Workspace::new(vec![bad], false)?;
    let mut root = FakeHost::root();
    let digest = run(&tampered.config, &mut root, &mut reporter)
        .err()
        .ok_or("摘要不符应失败")?;
    ensure_eq(&digest.code, &"plan_sha256", "错误码")?;
    Ok(())
}

/// 最终事件在窗口内未送达：不重启、不禁用服务。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn undelivered_final_event_blocks_reboot() -> TestResult {
    let ws = Workspace::new(vec![entry("alpha", "true\n", 30)], true)?;
    let mut host = FakeHost::root();
    let mut reporter = Recorder {
        down: true,
        ..Recorder::default()
    };
    let outcome = run(&ws.config, &mut host, &mut reporter)?;
    ensure_eq(
        &outcome,
        &RunOutcome::Undelivered {
            kind: "agent.finished".to_owned(),
        },
        "未送达",
    )?;
    ensure_eq(&host.reboots, &0, "未送达不重启")?;
    ensure(
        host.disabled.is_empty(),
        "未送达不禁用服务（下次启动续跑重报）",
    )?;
    ensure(exists(&ws.config.plan_path), "计划保留")?;
    ensure(
        host.slept <= Duration::from_secs(15 + 60),
        "进度事件只在首次退避，最终事件不超过窗口",
    )?;
    ensure(host.slept >= Duration::from_secs(30), "最终事件持续重试")?;
    Ok(())
}

/// 瞬时网络故障按指数退避重试。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn transient_failures_are_retried() -> TestResult {
    let ws = Workspace::new(vec![entry("alpha", "true\n", 30)], false)?;
    let mut host = FakeHost::root();
    let mut reporter = Recorder {
        fail_first: 2,
        ..Recorder::default()
    };
    ensure_eq(
        &run(&ws.config, &mut host, &mut reporter)?,
        &RunOutcome::Finished { rebooted: false },
        "完成",
    )?;
    ensure_eq(&reporter.events.len(), &4, "全部事件送达")?;
    ensure_eq(&host.slept, &Duration::from_secs(3), "退避 1s + 2s")?;
    Ok(())
}

/// 起一个只接受一次请求的 HTTP 服务，返回地址与收到的请求。
///
/// # Errors
///
/// 绑定端口失败时返回错误。
fn one_shot(status: &'static str) -> std::io::Result<(String, thread::JoinHandle<String>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let Ok((stream, _peer)) = listener.accept() else {
            return String::new();
        };
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        let mut length = 0_usize;
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|count| count > 2) {
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
            head.push_str(&line);
            line.clear();
        }
        let mut body = vec![0_u8; length];
        let _read = reader.read_exact(&mut body);
        let _reply = reader
            .get_mut()
            .write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes());
        format!("{head}\r\n{}", String::from_utf8_lossy(&body))
    });
    Ok((format!("http://{addr}/p/agent001/events"), handle))
}

/// 最小 HTTP 客户端：请求格式、非 2xx、拒绝 https。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn http_reporter_posts_json() -> TestResult {
    let (url, server) = one_shot("204 No Content")?;
    let mut reporter = HttpReporter::new(&url)?;
    let event = AgentEvent::new(
        "sess01",
        "agent.started",
        None,
        serde_json::json!({ "modules": 2_u32 }),
    );
    reporter.send(&event)?;
    let request = server.join().map_err(|_panic| "服务线程异常")?;
    ensure(
        request.starts_with("POST /p/agent001/events HTTP/1.1\r\n"),
        "请求行",
    )?;
    ensure(
        request.contains("Content-Type: application/json"),
        "JSON 头",
    )?;
    let body = request.split("\r\n\r\n").nth(1).ok_or("缺正文")?;
    let parsed: serde_json::Value = serde_json::from_str(body)?;
    ensure_eq(
        &parsed.get("event_id").and_then(serde_json::Value::as_str),
        &Some("sess01.agent.started"),
        "event_id",
    )?;
    ensure_eq(
        &parsed
            .pointer("/payload/modules")
            .and_then(serde_json::Value::as_u64),
        &Some(2),
        "payload",
    )?;
    let (bad_url, bad_server) = one_shot("500 Internal Server Error")?;
    let failed = HttpReporter::new(&bad_url)?.send(&event);
    let _request = bad_server.join().map_err(|_panic| "服务线程异常")?;
    ensure(failed.is_err(), "非 2xx 视为失败")?;
    ensure(
        HttpReporter::new("https://example/x").is_err(),
        "拒绝 https",
    )?;
    Ok(())
}
