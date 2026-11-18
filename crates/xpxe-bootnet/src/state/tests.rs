//! `state` 模块单元测试：计数、观测队列、健康状态与心跳。

use std::net::Ipv4Addr;

use xpxe_domain::{Health, Observation, ProbeReport, Timestamp};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{BATCH_SIZE, Counter, Shared};

/// 一条模式切换观测。
fn mode(at: u64) -> Observation {
    Observation::ModeChanged {
        at: Timestamp::from_unix_seconds(at),
        from: "a".to_owned(),
        to: "b".to_owned(),
    }
}

/// 计数与队列。
///
/// # Errors
///
/// 断言失败。
#[test]
fn counters_and_queue() -> TestResult {
    let shared = Shared::default();
    shared.count(Counter::DhcpReplies);
    shared.count(Counter::DhcpReplies);
    shared.count(Counter::TftpFailed);
    let counters = shared.counters();
    ensure_eq(
        &(counters.dhcp_replies, counters.tftp_failed),
        &(2, 1),
        "计数",
    )?;
    for at in 0..300 {
        shared.observe(mode(at));
    }
    let batch = shared.take_batch();
    ensure_eq(&batch.len(), &BATCH_SIZE, "一批 200")?;
    shared.requeue(batch);
    let again = shared.take_batch();
    ensure_eq(&again.first(), &Some(&mode(0)), "放回队首保持顺序")?;
    ensure(shared.running(), "默认运行")?;
    shared.request_stop();
    ensure(!shared.running(), "请求停止")?;
    Ok(())
}

/// 健康：ok → degraded → conflict → ok。
///
/// # Errors
///
/// 断言失败。
#[test]
fn health_transitions() -> TestResult {
    let shared = Shared::default();
    ensure_eq(&shared.health().0, &Health::Ok, "初始 ok")?;
    shared.set_problem("control", "lost".to_owned());
    ensure_eq(&shared.health().0, &Health::Degraded, "降级")?;
    shared.clear_problem("control");
    let report = ProbeReport {
        at: Timestamp::from_unix_seconds(1),
        responders: Vec::new(),
        conflicts: vec![Ipv4Addr::new(10, 77, 0, 3)],
        external: None,
    };
    ensure(shared.set_probe(report, true), "暂停")?;
    let (health, detail) = shared.health();
    ensure_eq(&health, &Health::Conflict, "冲突")?;
    ensure(
        detail.is_some_and(|text| text.contains("10.77.0.3")),
        "说明含冲突服务器",
    )?;
    shared.resume();
    ensure(!shared.paused(), "恢复")?;
    shared.set_applied(7, "proxy_dhcp");
    shared.set_listening(vec![67, 4011, 69]);
    let beat = shared.heartbeat(Timestamp::from_unix_seconds(9));
    ensure_eq(
        &(beat.rev, beat.mode.as_str(), beat.listening.len()),
        &(7, "proxy_dhcp", 3),
        "心跳内容",
    )?;
    ensure(beat.probe.is_some(), "心跳带探测结论")?;
    Ok(())
}
