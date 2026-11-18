//! 定时器：按最近的 `deadline_at` 休眠（上限为轮询间隔），到期投递 `DeadlineExpired`。

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use xpxe_domain::{EventEnvelope, EventSource, NodeEvent};

use crate::Outcome;
use crate::app::App;
use crate::auth::Actor;
use crate::transition::{Incoming, deliver, describe, fresh_id};

/// 投递所有已到期的超时，返回投递条数。
///
/// # Errors
///
/// 读取节点失败时返回错误。
pub(crate) fn fire_due(app: &App) -> Outcome<usize> {
    let now = app.now();
    let mut fired = 0_usize;
    for row in app.store.nodes().list()? {
        let Some(deadline) = row.deadline_at.filter(|at| *at <= now) else {
            continue;
        };
        let incoming = Incoming {
            node_id: row.id,
            envelope: EventEnvelope {
                id: fresh_id(app, "timer.deadline", row.id)?,
                session: None,
                source: EventSource::Timer,
                event: NodeEvent::DeadlineExpired {
                    state: row.state,
                    since: row.state_since,
                },
            },
            actor: Actor::system(),
            payload: json!({ "deadline_at": deadline }),
        };
        match deliver(app, &incoming) {
            Ok(_delivered) => fired = fired.saturating_add(1),
            Err(error) => tracing::warn!(detail = %describe(&error, row.id), "超时事件被拒"),
        }
    }
    Ok(fired)
}

/// 最近的截止时间距今的等待时长（上限为轮询间隔）。
fn next_wait(app: &App) -> Duration {
    let cap = app.options.poll;
    let now = app.now().unix_seconds();
    let nearest = app
        .store
        .nodes()
        .list()
        .ok()
        .and_then(|rows| rows.iter().filter_map(|row| row.deadline_at).min());
    nearest.map_or(cap, |at| {
        Duration::from_secs(at.unix_seconds().saturating_sub(now)).min(cap)
    })
}

/// 调度主循环。
pub(crate) async fn run(app: Arc<App>, mut stop: tokio::sync::watch::Receiver<bool>) {
    while !*stop.borrow() {
        if let Err(error) = fire_due(&app) {
            tracing::error!(%error, "定时器处理失败");
        }
        let wait = next_wait(&app).max(Duration::from_millis(100));
        tokio::select! {
            _woken = tokio::time::timeout(wait, app.timers_wake.notified()) => {}
            _changed = stop.changed() => {}
        }
    }
}
