//! 推送中心补发。

use serde_json::json;
use xpxe_test_support::{TestResult, ensure_eq};

use super::Hub;

/// `Last-Event-ID` 之后的事件被补发，之前的不补发。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn replays_after_last_event_id() -> TestResult {
    let hub = Hub::new();
    hub.publish("a", json!(1_i32));
    hub.publish("b", json!(2_i32));
    hub.publish("c", json!(3_i32));
    let (replay, _receiver) = hub.subscribe(Some(1));
    let ids: Vec<u64> = replay.iter().map(|pushed| pushed.id).collect();
    ensure_eq(&ids, &vec![2, 3], "补发 2、3")?;
    let (none, _other) = hub.subscribe(None);
    ensure_eq(&none.len(), &0, "无 ID 不补发")?;
    Ok(())
}
