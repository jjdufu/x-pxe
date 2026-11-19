//! 错误格式与映射。

use http::StatusCode;
use xpxe_domain::{NodeState, TransitionError};
use xpxe_store::StoreError;
use xpxe_test_support::{TestResult, ensure_eq};

use super::{Failure, FieldIssue};

/// 存储与状态机错误映射到稳定的 HTTP 状态与错误码。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn errors_map_to_status_and_code() -> TestResult {
    let conflict = Failure::from(StoreError::Conflict { current_head: 3 });
    ensure_eq(&conflict.status(), &StatusCode::CONFLICT, "并发修改 409")?;
    ensure_eq(&conflict.code(), &"VERSION_CONFLICT", "错误码")?;
    let gone = Failure::from(StoreError::AlreadyServed);
    ensure_eq(&gone.status(), &StatusCode::GONE, "一次性文档 410")?;
    let stale = Failure::from(TransitionError::StaleSession);
    ensure_eq(&stale.code(), &"STALE_SESSION", "会话不匹配")?;
    let changed = Failure::from(StoreError::StateChanged {
        current: NodeState::Ready,
    });
    ensure_eq(&changed.status(), &StatusCode::CONFLICT, "状态竞争 409")?;
    let invalid = Failure::invalid(vec![FieldIssue::new("/a", "bad", "坏")]);
    ensure_eq(&invalid.fields().len(), &1, "字段错误")?;
    Ok(())
}

/// 镜像内 agent 摘要复核：无摘要文件放行，摘要一致通过，不一致拒绝启动。
///
/// # Errors
///
/// 断言失败或临时文件写入失败时返回错误。
#[test]
fn agent_digest_is_verified_when_sidecar_present() -> TestResult {
    let dir = xpxe_test_support::TempDataDir::new()?;
    let agent = dir.path().join("xpxe-agent");
    std::fs::write(&agent, b"agent-bytes")?;
    ensure_eq(
        &crate::app::verify_agent_digest(&agent).ok(),
        &Some(None),
        "无摘要文件",
    )?;
    let digest = xpxe_render::init::sha256_hex(b"agent-bytes");
    std::fs::write(
        dir.path().join("xpxe-agent.sha256"),
        format!("{digest}  xpxe-agent\n"),
    )?;
    ensure_eq(
        &crate::app::verify_agent_digest(&agent).ok(),
        &Some(Some(digest)),
        "摘要一致",
    )?;
    std::fs::write(&agent, b"tampered")?;
    let refused = crate::app::verify_agent_digest(&agent).err();
    ensure_eq(
        &refused.as_ref().map(Failure::code),
        &Some("AGENT_DIGEST_MISMATCH"),
        "摘要不一致拒绝",
    )?;
    Ok(())
}
