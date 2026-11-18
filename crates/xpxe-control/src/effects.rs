//! 发件箱 worker：发布指令、Redfish 弹出介质、关闭会话（作废令牌、删除 ISO 缓存）；失败指数退避。

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use xpxe_domain::{BootChannel, SessionId, Timestamp};
use xpxe_store::{EventRecord, OutboxEntry};

use crate::app::App;
use crate::artifacts;
use crate::bmc::eject_node;
use crate::nodes::load;
use crate::{Failure, Outcome, internal};

/// 退避上限（秒）。
const BACKOFF_CAP: u64 = 300;

/// 第 `attempts` 次失败后的等待秒数：2^n，上限 300。
pub(crate) fn backoff(attempts: u32) -> u64 {
    2_u64
        .checked_pow(attempts.saturating_add(1))
        .map_or(BACKOFF_CAP, |secs| secs.min(BACKOFF_CAP))
}

/// 关闭会话：作废令牌、删除令牌原文与 ISO 缓存、发布指令。
///
/// # Errors
///
/// 写入失败时返回错误。
fn close_session(app: &App, entry: &OutboxEntry) -> Outcome<()> {
    let Some(raw) = entry.session_id.as_deref() else {
        return Ok(());
    };
    let session = SessionId::parse(raw).map_err(internal)?;
    let outcome = entry
        .payload
        .get("outcome")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("closed");
    let _closed = app.store.tokens().close(&session, outcome, app.now())?;
    artifacts::forget_tokens(&app.settings, raw);
    let iso = app.settings.iso_dir().join(format!("{raw}.iso"));
    if let Err(error) = std::fs::remove_file(&iso)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(internal(error));
    }
    let _first_ip = app.volatile().boot_ips.remove(raw);
    app.bump_directives();
    Ok(())
}

/// 执行一条副作用。
///
/// # Errors
///
/// 执行失败时返回错误（由调用方退避重试）。
async fn execute(app: &App, entry: &OutboxEntry) -> Outcome<()> {
    match entry.effect.as_str() {
        "publish_directives" => {
            app.bump_directives();
            Ok(())
        }
        "close_session" => close_session(app, entry),
        "eject_media" => {
            let (_row, doc) = load(&app.store, entry.node_id)?;
            if matches!(doc.spec.channel(), BootChannel::Redfish { .. }) {
                eject_node(app, entry.node_id).await
            } else {
                Ok(())
            }
        }
        other => Err(Failure::internal(format!("未知副作用 {other}"))),
    }
}

/// 把副作用结果记为节点事件（状态不变）。
fn note(app: &App, entry: &OutboxEntry, ok: bool, detail: &str) {
    let Ok(row) = app.store.nodes().get(entry.node_id) else {
        return;
    };
    let Ok(event_id) = xpxe_domain::EventId::parse(&format!("fx.{}.{}", entry.id, entry.attempts))
    else {
        return;
    };
    let record = EventRecord {
        event_id,
        session_id: entry
            .session_id
            .as_deref()
            .and_then(|raw| SessionId::parse(raw).ok()),
        at: app.now(),
        source: "system".to_owned(),
        kind: format!("effect.{}", entry.effect),
        from_state: row.state,
        to_state: row.state,
        deadline_at: row.deadline_at,
        active_session_id: row.active_session_id,
        payload: json!({ "ok": ok, "detail": detail, "attempt": entry.attempts.saturating_add(1) }),
    };
    if let Err(error) = app.store.nodes().record(entry.node_id, &record) {
        tracing::warn!(%error, "记录副作用结果失败");
    }
    app.push(
        "task.progress",
        json!({ "node_id": entry.node_id, "task": entry.effect, "ok": ok, "detail": detail }),
    );
}

/// 处理所有到期条目，返回处理条数。
///
/// # Errors
///
/// 读取发件箱失败时返回错误。
pub(crate) async fn drain(app: &App) -> Outcome<usize> {
    let due = app.store.outbox().due(app.now(), 50)?;
    for entry in &due {
        match execute(app, entry).await {
            Ok(()) => {
                app.store.outbox().complete(entry.id, app.now())?;
                if entry.effect == "eject_media" {
                    note(app, entry, true, "已弹出虚拟介质");
                }
            }
            Err(error) => {
                let wait = backoff(entry.attempts);
                let next = app.now().saturating_add(Duration::from_secs(wait));
                app.store.outbox().defer(entry.id, error.message(), next)?;
                note(app, entry, false, error.message());
                tracing::warn!(effect = %entry.effect, node = entry.node_id, %error, wait, "副作用失败，退避重试");
            }
        }
    }
    Ok(due.len())
}

/// 距下一条到期的等待时长（上限为轮询间隔）。
fn wait_for(app: &App, next: Option<Timestamp>) -> Duration {
    let cap = app.options.poll;
    next.map_or(cap, |at| {
        let secs = at.unix_seconds().saturating_sub(app.now().unix_seconds());
        Duration::from_secs(secs).min(cap)
    })
}

/// worker 主循环。
pub(crate) async fn run(app: Arc<App>, mut stop: tokio::sync::watch::Receiver<bool>) {
    while !*stop.borrow() {
        if let Err(error) = drain(&app).await {
            tracing::error!(%error, "发件箱处理失败");
        }
        let next = app.store.outbox().next_due().unwrap_or(None);
        let wait = wait_for(&app, next);
        tokio::select! {
            _woken = tokio::time::timeout(wait.max(Duration::from_millis(10)), app.effects_wake.notified()) => {}
            _changed = stop.changed() => {}
        }
    }
}
