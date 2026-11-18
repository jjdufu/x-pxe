//! 状态机：7×11 全组合、会话校验、EventId 幂等、迟到超时与 Rearm 确认。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{
    AuditKind, Effect, EventEnvelope, EventId, EventSource, FailReason, NodeEvent, NodeEventKind,
    NodeRuntime, NodeState, SessionId, Timestamp, Transition, TransitionError, Ttl, apply,
};
use crate::value::{Hostname, MacAddr};

/// 测试用起始时间。
const T0: Timestamp = Timestamp::from_unix_seconds(1_000);

/// 期望的转换结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// 进入该状态。
    To(NodeState),
    /// `NotAllowed`。
    NotAllowed,
    /// `StaleSession`。
    StaleSession,
    /// `StaleDeadline`。
    StaleDeadline,
}

/// 7 个状态 × 11 个事件，使用“有效载荷”（匹配会话、当前 since、正确主机名）。
const CASES: &[(NodeState, NodeEventKind, Expect)] = &[
    (
        NodeState::Idle,
        NodeEventKind::Arm,
        Expect::To(NodeState::Armed),
    ),
    (NodeState::Idle, NodeEventKind::Disarm, Expect::NotAllowed),
    (NodeState::Idle, NodeEventKind::Rearm, Expect::NotAllowed),
    (NodeState::Idle, NodeEventKind::Reset, Expect::NotAllowed),
    (
        NodeState::Idle,
        NodeEventKind::InstallerStarted,
        Expect::StaleSession,
    ),
    (
        NodeState::Idle,
        NodeEventKind::InstallerFinished,
        Expect::StaleSession,
    ),
    (
        NodeState::Idle,
        NodeEventKind::InstallerFailed,
        Expect::StaleSession,
    ),
    (
        NodeState::Idle,
        NodeEventKind::AgentStarted,
        Expect::StaleSession,
    ),
    (
        NodeState::Idle,
        NodeEventKind::AgentFinished,
        Expect::StaleSession,
    ),
    (
        NodeState::Idle,
        NodeEventKind::AgentFailed,
        Expect::StaleSession,
    ),
    (
        NodeState::Idle,
        NodeEventKind::DeadlineExpired,
        Expect::StaleDeadline,
    ),
    (NodeState::Armed, NodeEventKind::Arm, Expect::NotAllowed),
    (
        NodeState::Armed,
        NodeEventKind::Disarm,
        Expect::To(NodeState::Idle),
    ),
    (NodeState::Armed, NodeEventKind::Rearm, Expect::NotAllowed),
    (NodeState::Armed, NodeEventKind::Reset, Expect::NotAllowed),
    (
        NodeState::Armed,
        NodeEventKind::InstallerStarted,
        Expect::To(NodeState::Installing),
    ),
    (
        NodeState::Armed,
        NodeEventKind::InstallerFinished,
        Expect::NotAllowed,
    ),
    (
        NodeState::Armed,
        NodeEventKind::InstallerFailed,
        Expect::NotAllowed,
    ),
    (
        NodeState::Armed,
        NodeEventKind::AgentStarted,
        Expect::NotAllowed,
    ),
    (
        NodeState::Armed,
        NodeEventKind::AgentFinished,
        Expect::NotAllowed,
    ),
    (
        NodeState::Armed,
        NodeEventKind::AgentFailed,
        Expect::NotAllowed,
    ),
    (
        NodeState::Armed,
        NodeEventKind::DeadlineExpired,
        Expect::To(NodeState::Idle),
    ),
    (
        NodeState::Installing,
        NodeEventKind::Arm,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installing,
        NodeEventKind::Disarm,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installing,
        NodeEventKind::Rearm,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installing,
        NodeEventKind::Reset,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installing,
        NodeEventKind::InstallerStarted,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installing,
        NodeEventKind::InstallerFinished,
        Expect::To(NodeState::Installed),
    ),
    (
        NodeState::Installing,
        NodeEventKind::InstallerFailed,
        Expect::To(NodeState::Failed),
    ),
    (
        NodeState::Installing,
        NodeEventKind::AgentStarted,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installing,
        NodeEventKind::AgentFinished,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installing,
        NodeEventKind::AgentFailed,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installing,
        NodeEventKind::DeadlineExpired,
        Expect::To(NodeState::Failed),
    ),
    (NodeState::Installed, NodeEventKind::Arm, Expect::NotAllowed),
    (
        NodeState::Installed,
        NodeEventKind::Disarm,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installed,
        NodeEventKind::Rearm,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installed,
        NodeEventKind::Reset,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installed,
        NodeEventKind::InstallerStarted,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installed,
        NodeEventKind::InstallerFinished,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installed,
        NodeEventKind::InstallerFailed,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installed,
        NodeEventKind::AgentStarted,
        Expect::To(NodeState::Configuring),
    ),
    (
        NodeState::Installed,
        NodeEventKind::AgentFinished,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installed,
        NodeEventKind::AgentFailed,
        Expect::NotAllowed,
    ),
    (
        NodeState::Installed,
        NodeEventKind::DeadlineExpired,
        Expect::To(NodeState::Failed),
    ),
    (
        NodeState::Configuring,
        NodeEventKind::Arm,
        Expect::NotAllowed,
    ),
    (
        NodeState::Configuring,
        NodeEventKind::Disarm,
        Expect::NotAllowed,
    ),
    (
        NodeState::Configuring,
        NodeEventKind::Rearm,
        Expect::NotAllowed,
    ),
    (
        NodeState::Configuring,
        NodeEventKind::Reset,
        Expect::NotAllowed,
    ),
    (
        NodeState::Configuring,
        NodeEventKind::InstallerStarted,
        Expect::NotAllowed,
    ),
    (
        NodeState::Configuring,
        NodeEventKind::InstallerFinished,
        Expect::NotAllowed,
    ),
    (
        NodeState::Configuring,
        NodeEventKind::InstallerFailed,
        Expect::NotAllowed,
    ),
    (
        NodeState::Configuring,
        NodeEventKind::AgentStarted,
        Expect::NotAllowed,
    ),
    (
        NodeState::Configuring,
        NodeEventKind::AgentFinished,
        Expect::To(NodeState::Ready),
    ),
    (
        NodeState::Configuring,
        NodeEventKind::AgentFailed,
        Expect::To(NodeState::Failed),
    ),
    (
        NodeState::Configuring,
        NodeEventKind::DeadlineExpired,
        Expect::To(NodeState::Failed),
    ),
    (NodeState::Ready, NodeEventKind::Arm, Expect::NotAllowed),
    (NodeState::Ready, NodeEventKind::Disarm, Expect::NotAllowed),
    (
        NodeState::Ready,
        NodeEventKind::Rearm,
        Expect::To(NodeState::Armed),
    ),
    (NodeState::Ready, NodeEventKind::Reset, Expect::NotAllowed),
    (
        NodeState::Ready,
        NodeEventKind::InstallerStarted,
        Expect::StaleSession,
    ),
    (
        NodeState::Ready,
        NodeEventKind::InstallerFinished,
        Expect::StaleSession,
    ),
    (
        NodeState::Ready,
        NodeEventKind::InstallerFailed,
        Expect::StaleSession,
    ),
    (
        NodeState::Ready,
        NodeEventKind::AgentStarted,
        Expect::StaleSession,
    ),
    (
        NodeState::Ready,
        NodeEventKind::AgentFinished,
        Expect::StaleSession,
    ),
    (
        NodeState::Ready,
        NodeEventKind::AgentFailed,
        Expect::StaleSession,
    ),
    (
        NodeState::Ready,
        NodeEventKind::DeadlineExpired,
        Expect::StaleDeadline,
    ),
    (NodeState::Failed, NodeEventKind::Arm, Expect::NotAllowed),
    (NodeState::Failed, NodeEventKind::Disarm, Expect::NotAllowed),
    (
        NodeState::Failed,
        NodeEventKind::Rearm,
        Expect::To(NodeState::Armed),
    ),
    (
        NodeState::Failed,
        NodeEventKind::Reset,
        Expect::To(NodeState::Idle),
    ),
    (
        NodeState::Failed,
        NodeEventKind::InstallerStarted,
        Expect::StaleSession,
    ),
    (
        NodeState::Failed,
        NodeEventKind::InstallerFinished,
        Expect::StaleSession,
    ),
    (
        NodeState::Failed,
        NodeEventKind::InstallerFailed,
        Expect::StaleSession,
    ),
    (
        NodeState::Failed,
        NodeEventKind::AgentStarted,
        Expect::StaleSession,
    ),
    (
        NodeState::Failed,
        NodeEventKind::AgentFinished,
        Expect::StaleSession,
    ),
    (
        NodeState::Failed,
        NodeEventKind::AgentFailed,
        Expect::StaleSession,
    ),
    (
        NodeState::Failed,
        NodeEventKind::DeadlineExpired,
        Expect::StaleDeadline,
    ),
];

/// 把运行态推进到目标状态。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn at(state: NodeState) -> TestResult<NodeRuntime> {
    let idle = NodeRuntime::idle(
        Hostname::parse("node1")?,
        MacAddr::parse("52:54:00:77:01:b3")?,
        T0,
    );
    match state {
        NodeState::Idle => Ok(idle),
        NodeState::Armed => go(&idle, NodeEventKind::Arm),
        NodeState::Installing => go(
            &go(&idle, NodeEventKind::Arm)?,
            NodeEventKind::InstallerStarted,
        ),
        NodeState::Installed => {
            let installing = go(
                &go(&idle, NodeEventKind::Arm)?,
                NodeEventKind::InstallerStarted,
            )?;
            go(&installing, NodeEventKind::InstallerFinished)
        }
        NodeState::Configuring => at_configuring(),
        NodeState::Ready => go(&at_configuring()?, NodeEventKind::AgentFinished),
        NodeState::Failed => {
            let installing = go(
                &go(&idle, NodeEventKind::Arm)?,
                NodeEventKind::InstallerStarted,
            )?;
            go(&installing, NodeEventKind::InstallerFailed)
        }
    }
}

/// 到达 `Configuring`。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn at_configuring() -> TestResult<NodeRuntime> {
    let installed = at(NodeState::Installed)?;
    go(&installed, NodeEventKind::AgentStarted)
}

/// 应用一种事件并提交。
///
/// # Errors
///
/// 转换失败时返回错误。
fn go(current: &NodeRuntime, kind: NodeEventKind) -> TestResult<NodeRuntime> {
    let envelope = event_for(current, kind)?;
    let transition = apply(current, &envelope, T0, &Ttl::standard())?;
    Ok(current.commit(&envelope, T0, &transition))
}

/// 为表驱动测试构造信封。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn event_for(runtime: &NodeRuntime, kind: NodeEventKind) -> TestResult<EventEnvelope> {
    let session = session_for(runtime, kind)?;
    let event = node_event(runtime, kind);
    Ok(EventEnvelope {
        id: EventId::parse(&format!("e-{}-{}", runtime.state().as_str(), kind.as_str()))?,
        session,
        source: source_of(kind),
        event,
    })
}

/// 按事件种类选择会话。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn session_for(runtime: &NodeRuntime, kind: NodeEventKind) -> TestResult<Option<SessionId>> {
    match kind {
        NodeEventKind::Arm | NodeEventKind::Rearm => Ok(Some(SessionId::parse("sess-new")?)),
        NodeEventKind::InstallerStarted
        | NodeEventKind::InstallerFinished
        | NodeEventKind::InstallerFailed
        | NodeEventKind::AgentStarted
        | NodeEventKind::AgentFinished
        | NodeEventKind::AgentFailed => Ok(runtime
            .session()
            .cloned()
            .or(Some(SessionId::parse("sess-x")?))),
        NodeEventKind::Disarm | NodeEventKind::Reset | NodeEventKind::DeadlineExpired => {
            Ok(runtime.session().cloned())
        }
    }
}

/// 按种类构造事件载荷。
fn node_event(runtime: &NodeRuntime, kind: NodeEventKind) -> NodeEvent {
    match kind {
        NodeEventKind::Arm => NodeEvent::Arm,
        NodeEventKind::Disarm => NodeEvent::Disarm,
        NodeEventKind::Rearm => NodeEvent::Rearm {
            confirmation: "node1".to_owned(),
        },
        NodeEventKind::Reset => NodeEvent::Reset,
        NodeEventKind::InstallerStarted => NodeEvent::InstallerStarted,
        NodeEventKind::InstallerFinished => NodeEvent::InstallerFinished,
        NodeEventKind::InstallerFailed => NodeEvent::InstallerFailed,
        NodeEventKind::AgentStarted => NodeEvent::AgentStarted,
        NodeEventKind::AgentFinished => NodeEvent::AgentFinished,
        NodeEventKind::AgentFailed => NodeEvent::AgentFailed,
        NodeEventKind::DeadlineExpired => NodeEvent::DeadlineExpired {
            state: runtime.state(),
            since: runtime.state_since(),
        },
    }
}

/// 事件来源。
const fn source_of(kind: NodeEventKind) -> EventSource {
    match kind {
        NodeEventKind::Arm
        | NodeEventKind::Disarm
        | NodeEventKind::Rearm
        | NodeEventKind::Reset => EventSource::Admin,
        NodeEventKind::InstallerStarted
        | NodeEventKind::InstallerFinished
        | NodeEventKind::InstallerFailed => EventSource::Installer,
        NodeEventKind::AgentStarted | NodeEventKind::AgentFinished | NodeEventKind::AgentFailed => {
            EventSource::Agent
        }
        NodeEventKind::DeadlineExpired => EventSource::Timer,
    }
}

/// 比对实际结果与期望。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn check(
    from: NodeState,
    kind: NodeEventKind,
    expect: Expect,
    got: Result<Transition, TransitionError>,
) -> TestResult {
    let label = format!("{from} + {}", kind.as_str());
    match (expect, got) {
        (Expect::To(state), Ok(transition)) => Ok(ensure_eq(&transition.to, &state, &label)?),
        (
            Expect::NotAllowed,
            Err(TransitionError::NotAllowed {
                from: found_state,
                event,
            }),
        ) => {
            ensure_eq(&found_state, &from, &label)?;
            Ok(ensure_eq(&event, &kind, &label)?)
        }
        (Expect::StaleSession, Err(TransitionError::StaleSession))
        | (Expect::StaleDeadline, Err(TransitionError::StaleDeadline)) => Ok(()),
        (other_expect, other_got) => {
            Err(format!("{label}：期望 {other_expect:?}，得到 {other_got:?}").into())
        }
    }
}

/// 77 个状态×事件组合全部有显式断言。
///
/// # Errors
///
/// 任一组合与期望不符或数量不是 77 时返回错误。
#[test]
fn all_state_event_combinations() -> TestResult {
    let mut seen = 0_usize;
    for &(from, kind, expect) in CASES {
        let runtime = at(from)?;
        let envelope = event_for(&runtime, kind)?;
        let got = apply(&runtime, &envelope, T0, &Ttl::standard());
        check(from, kind, expect, got)?;
        seen = seen.saturating_add(1);
    }
    ensure_eq(&seen, &77_usize, "必须覆盖 77 种组合")?;
    ensure_eq(&CASES.len(), &77_usize, "表长度")?;
    Ok(())
}

/// 安装器事件会话不匹配时返回 `StaleSession`，状态不变。
///
/// # Errors
///
/// 未拒绝或不匹配的会话被接受时返回错误。
#[test]
fn installer_stale_or_missing_session() -> TestResult {
    let armed = at(NodeState::Armed)?;
    let mut envelope = event_for(&armed, NodeEventKind::InstallerStarted)?;
    envelope.session = Some(SessionId::parse("other")?);
    ensure_eq(
        &apply(&armed, &envelope, T0, &Ttl::standard()),
        &Err(TransitionError::StaleSession),
        "错误会话",
    )?;
    envelope.session = None;
    ensure_eq(
        &apply(&armed, &envelope, T0, &Ttl::standard()),
        &Err(TransitionError::StaleSession),
        "缺失会话",
    )?;
    ensure_eq(&armed.state(), &NodeState::Armed, "状态不变")?;
    Ok(())
}

/// 同一 `EventId` 重复提交返回首次结果。
///
/// # Errors
///
/// 二次提交结果不一致时返回错误。
#[test]
fn event_id_is_idempotent() -> TestResult {
    let idle = at(NodeState::Idle)?;
    let envelope = event_for(&idle, NodeEventKind::Arm)?;
    let first = apply(&idle, &envelope, T0, &Ttl::standard())?;
    let armed = idle.commit(&envelope, T0, &first);
    let again = apply(&armed, &envelope, T0, &Ttl::standard())?;
    ensure_eq(&again, &first, "幂等")?;
    ensure_eq(&armed.state(), &NodeState::Armed, "只提交一次")?;
    Ok(())
}

/// `state` 或 `since` 不匹配的超时事件被丢弃。
///
/// # Errors
///
/// 迟到超时被接受时返回错误。
#[test]
fn late_deadline_is_discarded() -> TestResult {
    let armed = at(NodeState::Armed)?;
    let since = armed.state_since();
    let installing = go(&armed, NodeEventKind::InstallerStarted)?;
    let envelope = EventEnvelope {
        id: EventId::parse("late-arm")?,
        session: None,
        source: EventSource::Timer,
        event: NodeEvent::DeadlineExpired {
            state: NodeState::Armed,
            since,
        },
    };
    ensure_eq(
        &apply(&installing, &envelope, T0, &Ttl::standard()),
        &Err(TransitionError::StaleDeadline),
        "迟到超时",
    )?;
    ensure_eq(
        &installing.state(),
        &NodeState::Installing,
        "保持 Installing",
    )?;
    Ok(())
}

/// `Ready` 上确认串必须与主机名一致；`Failed` 无需确认匹配也可重新武装。
///
/// # Errors
///
/// 确认逻辑不符时返回错误。
#[test]
fn rearm_confirmation() -> TestResult {
    let ready = at(NodeState::Ready)?;
    let mut envelope = event_for(&ready, NodeEventKind::Rearm)?;
    envelope.event = NodeEvent::Rearm {
        confirmation: "wrong".to_owned(),
    };
    ensure_eq(
        &apply(&ready, &envelope, T0, &Ttl::standard()),
        &Err(TransitionError::BadConfirmation),
        "错误确认",
    )?;
    let failed = at(NodeState::Failed)?;
    ensure_eq(
        &failed.fail_reason(),
        &Some(FailReason::Installer),
        "失败原因",
    )?;
    let ok = go(&failed, NodeEventKind::Rearm)?;
    ensure_eq(&ok.state(), &NodeState::Armed, "Failed 可 rearm")?;
    Ok(())
}

/// 武装副作用包含开会话与发布指令。
///
/// # Errors
///
/// 副作用不符时返回错误。
#[test]
fn arm_effects() -> TestResult {
    let idle = at(NodeState::Idle)?;
    let envelope = event_for(&idle, NodeEventKind::Arm)?;
    let transition = apply(&idle, &envelope, T0, &Ttl::standard())?;
    ensure(
        transition.effects.contains(&Effect::OpenSession)
            && transition.effects.contains(&Effect::PublishDirectives)
            && transition
                .effects
                .contains(&Effect::Audit(AuditKind::Armed)),
        "武装副作用",
    )?;
    ensure(transition.deadline.is_some(), "Armed 必须有截止时间")?;
    Ok(())
}

/// 从持久化字段恢复的运行态与原运行态行为一致。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn restore_matches_persisted_runtime() -> TestResult {
    let armed = at(NodeState::Armed)?;
    let restored = NodeRuntime::restore(
        (armed.hostname().clone(), armed.mac()),
        super::Persisted {
            state: armed.state(),
            since: armed.state_since(),
            deadline: armed.deadline(),
            session: armed.session().cloned(),
        },
    );
    ensure_eq(&restored.state(), &NodeState::Armed, "状态")?;
    ensure_eq(&restored.session(), &armed.session(), "会话")?;
    let next = go(&restored, NodeEventKind::InstallerStarted)?;
    ensure_eq(&next.state(), &NodeState::Installing, "可继续转换")?;
    Ok(())
}
