//! 事件正文解析与分类。

use xpxe_store::TokenKind;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{classify, event_id, parse_body};

/// 安装器 `wget --post-data` 文本与 agent JSON 都能解析；ID 按种类加后缀。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn parses_text_and_json() -> TestResult {
    let text = parse_body(b"event=installer.started event_id=s1.installer", None)?;
    ensure_eq(&text.kind.as_str(), &"installer.started", "文本种类")?;
    ensure_eq(
        &event_id(&text)?.as_str(),
        &"s1.installer.installer.started",
        "加后缀",
    )?;
    let failed = parse_body(
        b"event=installer.failed event_id=s1.installer reason=disk_selection inventory=x",
        Some("installer.failed"),
    )?;
    ensure_eq(
        &failed
            .payload
            .get("reason")
            .and_then(serde_json::Value::as_str),
        &Some("disk_selection"),
        "附加字段",
    )?;
    let agent = parse_body(
        br#"{"event_id":"s1.agent.started","kind":"agent.started","payload":{}}"#,
        None,
    )?;
    ensure_eq(
        &event_id(&agent)?.as_str(),
        &"s1.agent.started",
        "已含种类不加后缀",
    )?;
    ensure_eq(
        &classify("agent.finished")?.0,
        &TokenKind::Agent,
        "agent 令牌",
    )?;
    ensure_eq(
        &classify("installer.started")?.0,
        &TokenKind::Boot,
        "引导令牌",
    )?;
    ensure(classify("module.started")?.1.is_none(), "模块事件只记进度")?;
    ensure(classify("evil").is_err(), "白名单")?;
    Ok(())
}
