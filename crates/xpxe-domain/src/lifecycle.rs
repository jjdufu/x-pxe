//! 节点状态机：转换表、会话校验、事件幂等与超时。

use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::value::{Hostname, MacAddr, ParseError};

/// 由调用方传入的 Unix 秒时间戳，领域层不读时钟。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp {
    /// Unix 秒（UTC）。
    unix_seconds: u64,
}

impl Timestamp {
    /// 由 Unix 秒构造。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn from_unix_seconds(unix_seconds: u64) -> Self {
        Self { unix_seconds }
    }

    /// Unix 秒。
    #[must_use]
    pub const fn unix_seconds(self) -> u64 {
        self.unix_seconds
    }

    /// 加上一段时长，溢出时饱和到 `u64::MAX`。
    #[must_use]
    pub const fn saturating_add(self, duration: Duration) -> Self {
        Self {
            unix_seconds: self.unix_seconds.saturating_add(duration.as_secs()),
        }
    }
}

/// 各状态默认超时。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ttl {
    /// `Armed` 超时。
    arm: Duration,
    /// `Installing` 超时。
    install: Duration,
    /// `Installed` 超时。
    firstboot: Duration,
    /// `Configuring` 超时。
    configure: Duration,
}

impl Ttl {
    /// 文档约定的默认值：2 h / 90 min / 30 min / 60 min。
    #[must_use]
    pub const fn standard() -> Self {
        Self::from_secs(7200, 5400, 1800, 3600)
    }

    /// 由各状态的秒数构造。
    #[must_use]
    pub const fn from_secs(arm: u64, install: u64, firstboot: u64, configure: u64) -> Self {
        Self {
            arm: Duration::from_secs(arm),
            install: Duration::from_secs(install),
            firstboot: Duration::from_secs(firstboot),
            configure: Duration::from_secs(configure),
        }
    }

    /// `Armed` 超时。
    #[must_use]
    pub const fn arm(self) -> Duration {
        self.arm
    }

    /// `Installing` 超时。
    #[must_use]
    pub const fn install(self) -> Duration {
        self.install
    }

    /// `Installed` 超时。
    #[must_use]
    pub const fn firstboot(self) -> Duration {
        self.firstboot
    }

    /// `Configuring` 超时。
    #[must_use]
    pub const fn configure(self) -> Duration {
        self.configure
    }

    /// 进入 `state` 时使用的超时；无超时的状态返回 `None`。
    #[must_use]
    pub const fn for_state(self, state: NodeState) -> Option<Duration> {
        match state {
            NodeState::Armed => Some(self.arm),
            NodeState::Installing => Some(self.install),
            NodeState::Installed => Some(self.firstboot),
            NodeState::Configuring => Some(self.configure),
            NodeState::Idle | NodeState::Ready | NodeState::Failed => None,
        }
    }
}

impl Default for Ttl {
    fn default() -> Self {
        Self::standard()
    }
}

/// 发送方生成的事件标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EventId(String);

impl EventId {
    /// 解析事件 ID：1–128 位字母、数字、`-`、`_`、`.`。
    ///
    /// # Errors
    ///
    /// 格式不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        parse_ident(raw, "EventId").map(Self)
    }

    /// ID 文本。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EventId {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<EventId> for String {
    fn from(value: EventId) -> Self {
        value.0
    }
}

impl FromStr for EventId {
    type Err = ParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Display for EventId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 装机会话标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionId(String);

impl SessionId {
    /// 解析会话 ID：1–128 位字母、数字、`-`、`_`、`.`。
    ///
    /// # Errors
    ///
    /// 格式不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        parse_ident(raw, "SessionId").map(Self)
    }

    /// ID 文本。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SessionId {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<SessionId> for String {
    fn from(value: SessionId) -> Self {
        value.0
    }
}

impl FromStr for SessionId {
    type Err = ParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Display for SessionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 解析事件/会话标识。
///
/// # Errors
///
/// 失败时返回对应错误。
fn parse_ident(raw: &str, typ: &'static str) -> Result<String, ParseError> {
    if raw.is_empty() {
        return Err(ParseError::new(typ, "empty"));
    }
    if raw.len() > 128 {
        return Err(ParseError::new(typ, "too_long"));
    }
    let ok = raw
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.');
    if ok {
        Ok(raw.to_owned())
    } else {
        Err(ParseError::new(typ, "bad_format"))
    }
}

/// 节点运行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// 未授权装机。
    Idle,
    /// 本次装机会话已创建，允许引导。
    Armed,
    /// 安装器已启动。
    Installing,
    /// 安装器完成，等待首次启动。
    Installed,
    /// agent 正在执行初始化模块。
    Configuring,
    /// 完成并锁定。
    Ready,
    /// 失败，需人工处理。
    Failed,
}

impl NodeState {
    /// 稳定的蛇形名称。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Armed => "armed",
            Self::Installing => "installing",
            Self::Installed => "installed",
            Self::Configuring => "configuring",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

impl Display for NodeState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 状态机事件（带载荷）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeEvent {
    /// 管理员武装节点。
    Arm,
    /// 管理员解除武装。
    Disarm,
    /// 从 `Ready`/`Failed` 重新武装，需主机名确认。
    Rearm {
        /// 用户输入的主机名确认串。
        confirmation: String,
    },
    /// 从 `Failed` 回到 `Idle`。
    Reset,
    /// 安装器已启动。
    InstallerStarted,
    /// 安装器成功结束。
    InstallerFinished,
    /// 安装器失败。
    InstallerFailed,
    /// agent 已启动。
    AgentStarted,
    /// agent 成功结束。
    AgentFinished,
    /// agent 失败。
    AgentFailed,
    /// 定时器到期。只有 `state` 与 `since` 都匹配当前值才生效。
    DeadlineExpired {
        /// 调度该定时器时的状态。
        state: NodeState,
        /// 调度该定时器时的 `state_since`。
        since: Timestamp,
    },
}

impl NodeEvent {
    /// 不带载荷的事件种类。
    #[must_use]
    pub const fn kind(&self) -> NodeEventKind {
        match self {
            Self::Arm => NodeEventKind::Arm,
            Self::Disarm => NodeEventKind::Disarm,
            Self::Rearm { .. } => NodeEventKind::Rearm,
            Self::Reset => NodeEventKind::Reset,
            Self::InstallerStarted => NodeEventKind::InstallerStarted,
            Self::InstallerFinished => NodeEventKind::InstallerFinished,
            Self::InstallerFailed => NodeEventKind::InstallerFailed,
            Self::AgentStarted => NodeEventKind::AgentStarted,
            Self::AgentFinished => NodeEventKind::AgentFinished,
            Self::AgentFailed => NodeEventKind::AgentFailed,
            Self::DeadlineExpired { .. } => NodeEventKind::DeadlineExpired,
        }
    }

    /// 是否来自安装器或 agent，因而必须携带并匹配当前会话。
    #[must_use]
    pub const fn requires_session(&self) -> bool {
        matches!(
            self,
            Self::InstallerStarted
                | Self::InstallerFinished
                | Self::InstallerFailed
                | Self::AgentStarted
                | Self::AgentFinished
                | Self::AgentFailed
        )
    }
}

/// 不带载荷的事件种类，用于错误与表驱动测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeEventKind {
    /// 武装。
    Arm,
    /// 解除武装。
    Disarm,
    /// 重新武装。
    Rearm,
    /// 复位。
    Reset,
    /// 安装器启动。
    InstallerStarted,
    /// 安装器结束。
    InstallerFinished,
    /// 安装器失败。
    InstallerFailed,
    /// agent 启动。
    AgentStarted,
    /// agent 结束。
    AgentFinished,
    /// agent 失败。
    AgentFailed,
    /// 超时。
    DeadlineExpired,
}

impl Display for NodeEventKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl NodeEventKind {
    /// 稳定的蛇形名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Arm => "arm",
            Self::Disarm => "disarm",
            Self::Rearm => "rearm",
            Self::Reset => "reset",
            Self::InstallerStarted => "installer_started",
            Self::InstallerFinished => "installer_finished",
            Self::InstallerFailed => "installer_failed",
            Self::AgentStarted => "agent_started",
            Self::AgentFinished => "agent_finished",
            Self::AgentFailed => "agent_failed",
            Self::DeadlineExpired => "deadline_expired",
        }
    }
}

/// 事件来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    /// 管理员（Web / API）。
    Admin,
    /// 安装器回报。
    Installer,
    /// 目标机 agent。
    Agent,
    /// 服务端定时器。
    Timer,
}

/// 投递给状态机的事件信封。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// 发送方生成的事件 ID，用于幂等。
    pub id: EventId,
    /// 会话。武装/重新武装时为新会话；安装器与 agent 必须等于当前会话。
    pub session: Option<SessionId>,
    /// 来源。
    pub source: EventSource,
    /// 事件。
    pub event: NodeEvent,
}

/// 一次成功转换的结果。领域层只描述要做什么，由控制面执行 [`Effect`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// 目标状态。
    pub to: NodeState,
    /// 新的截止时间；无超时则为 `None`。
    pub deadline: Option<Timestamp>,
    /// 控制面应当执行的副作用。
    pub effects: Vec<Effect>,
}

/// 控制面应当执行的副作用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    /// 打开新的装机会话（会话 ID 取自事件信封）。
    OpenSession,
    /// 关闭当前会话。
    CloseSession(Outcome),
    /// 向 bootnet 发布或撤销应答指令。
    PublishDirectives,
    /// 弹出 Redfish 虚拟介质。
    EjectMedia,
    /// 写审计。
    Audit(AuditKind),
}

/// 会话关闭时的结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// 成功就绪。
    Success,
    /// 失败。
    Failure,
    /// 管理员取消或武装超时回到 Idle。
    Cancelled,
    /// 超时失败。
    Timeout,
}

/// 失败原因（写入运行态，不进入 [`NodeState`] 以免破坏 7 状态枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailReason {
    /// 安装器失败。
    Installer,
    /// agent 失败。
    Agent,
    /// 超时。
    Timeout,
}

/// 审计种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditKind {
    /// 武装。
    Armed,
    /// 解除武装或武装超时。
    Disarmed,
    /// 安装开始。
    InstallStarted,
    /// 安装结束。
    InstallFinished,
    /// 初始化开始。
    AgentStarted,
    /// 节点就绪。
    Ready,
    /// 失败。
    Failed,
    /// 复位。
    Reset,
}

/// 转换被拒绝且状态不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    /// 不在转换表内的组合。
    NotAllowed {
        /// 当前状态。
        from: NodeState,
        /// 事件种类。
        event: NodeEventKind,
    },
    /// 安装器/agent 事件缺少会话，或与当前会话不一致。
    StaleSession,
    /// 迟到或不适用的 `DeadlineExpired`。
    StaleDeadline,
    /// `Rearm` 确认串与节点主机名不一致。
    BadConfirmation,
    /// 武装/重新武装未携带新会话 ID。
    MissingSession,
}

impl Display for TransitionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAllowed { from, event } => {
                write!(f, "不允许从 {from} 处理 {}", event.as_str())
            }
            Self::StaleSession => f.write_str("会话不匹配或缺失"),
            Self::StaleDeadline => f.write_str("迟到或不适用的超时事件"),
            Self::BadConfirmation => f.write_str("重新武装确认串与主机名不一致"),
            Self::MissingSession => f.write_str("武装事件必须携带新会话"),
        }
    }
}

impl std::error::Error for TransitionError {}

/// 持久化的运行态字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persisted {
    /// 当前状态。
    pub state: NodeState,
    /// 进入当前状态的时间。
    pub since: Timestamp,
    /// 当前截止时间。
    pub deadline: Option<Timestamp>,
    /// 活动会话。
    pub session: Option<SessionId>,
}

/// 节点运行态。领域层不存储密钥或令牌原文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRuntime {
    /// 主机名，用于 `Rearm` 确认。
    hostname: Hostname,
    /// 节点 MAC。
    mac: MacAddr,
    /// 当前状态。
    state: NodeState,
    /// 进入当前状态的时间。
    state_since: Timestamp,
    /// 当前截止时间。
    deadline: Option<Timestamp>,
    /// 活动会话。
    session: Option<SessionId>,
    /// 若处于 Failed，记录原因。
    fail_reason: Option<FailReason>,
    /// 最近一次已处理的事件 ID。
    last_event_id: Option<EventId>,
    /// 最近一次已处理事件的结果（用于幂等）。
    last_result: Option<Result<Transition, TransitionError>>,
}

impl NodeRuntime {
    /// 处于 `Idle`、无会话的新运行态。
    #[must_use]
    pub const fn idle(hostname: Hostname, mac: MacAddr, now: Timestamp) -> Self {
        Self {
            hostname,
            mac,
            state: NodeState::Idle,
            state_since: now,
            deadline: None,
            session: None,
            fail_reason: None,
            last_event_id: None,
            last_result: None,
        }
    }

    /// 由持久化的运行态恢复（控制面从数据库重建）；幂等由存储层的 `event_id` 去重保证。
    #[must_use]
    pub fn restore(identity: (Hostname, MacAddr), persisted: Persisted) -> Self {
        let (hostname, mac) = identity;
        Self {
            hostname,
            mac,
            state: persisted.state,
            state_since: persisted.since,
            deadline: persisted.deadline,
            session: persisted.session,
            fail_reason: None,
            last_event_id: None,
            last_result: None,
        }
    }

    /// 主机名。
    #[must_use]
    pub const fn hostname(&self) -> &Hostname {
        &self.hostname
    }

    /// MAC。
    #[must_use]
    pub const fn mac(&self) -> MacAddr {
        self.mac
    }

    /// 当前状态。
    #[must_use]
    pub const fn state(&self) -> NodeState {
        self.state
    }

    /// 进入当前状态的时间。
    #[must_use]
    pub const fn state_since(&self) -> Timestamp {
        self.state_since
    }

    /// 当前截止时间。
    #[must_use]
    pub const fn deadline(&self) -> Option<Timestamp> {
        self.deadline
    }

    /// 活动会话。
    #[must_use]
    pub const fn session(&self) -> Option<&SessionId> {
        self.session.as_ref()
    }

    /// 失败原因。
    #[must_use]
    pub const fn fail_reason(&self) -> Option<FailReason> {
        self.fail_reason
    }

    /// 把一次成功转换应用到本运行态，返回新的运行态。幂等重放应避免再次调用。
    #[must_use]
    pub fn commit(
        &self,
        envelope: &EventEnvelope,
        now: Timestamp,
        transition: &Transition,
    ) -> Self {
        apply_commit(self, envelope, now, transition)
    }
}

/// 将 [`Transition`] 写回运行态。
fn apply_commit(
    current: &NodeRuntime,
    envelope: &EventEnvelope,
    now: Timestamp,
    transition: &Transition,
) -> NodeRuntime {
    let state_changed = transition.to != current.state;
    let session = next_session(current.session.clone(), envelope, &transition.effects);
    NodeRuntime {
        hostname: current.hostname.clone(),
        mac: current.mac,
        state: transition.to,
        state_since: if state_changed {
            now
        } else {
            current.state_since
        },
        deadline: transition.deadline,
        session,
        fail_reason: fail_reason_after(transition, &envelope.event),
        last_event_id: Some(envelope.id.clone()),
        last_result: Some(Ok(transition.clone())),
    }
}

/// 根据副作用计算新的活动会话。
fn next_session(
    current: Option<SessionId>,
    envelope: &EventEnvelope,
    effects: &[Effect],
) -> Option<SessionId> {
    if effects
        .iter()
        .any(|effect| matches!(effect, Effect::CloseSession(_)))
    {
        return None;
    }
    if effects
        .iter()
        .any(|effect| matches!(effect, Effect::OpenSession))
    {
        return envelope.session.clone();
    }
    current
}

/// 进入 Failed 时记录原因。
fn fail_reason_after(transition: &Transition, event: &NodeEvent) -> Option<FailReason> {
    if transition.to != NodeState::Failed {
        return None;
    }
    match event {
        NodeEvent::InstallerFailed => Some(FailReason::Installer),
        NodeEvent::AgentFailed => Some(FailReason::Agent),
        NodeEvent::DeadlineExpired { .. } => Some(FailReason::Timeout),
        NodeEvent::Arm
        | NodeEvent::Disarm
        | NodeEvent::Rearm { .. }
        | NodeEvent::Reset
        | NodeEvent::InstallerStarted
        | NodeEvent::InstallerFinished
        | NodeEvent::AgentStarted
        | NodeEvent::AgentFinished => None,
    }
}

/// 应用事件。同一 `EventId` 重复提交时返回首次结果。
///
/// # Errors
///
/// 表外组合、会话不匹配、迟到的超时、错误的确认串或缺少新会话时返回 [`TransitionError`]。
pub fn apply(
    current: &NodeRuntime,
    event: &EventEnvelope,
    now: Timestamp,
    ttl: &Ttl,
) -> Result<Transition, TransitionError> {
    if let Some(previous) = replay(current, &event.id) {
        return previous;
    }
    if event.event.requires_session() {
        check_session(current, event.session.as_ref())?;
    }
    dispatch(current, event, now, ttl)
}

/// 若 `EventId` 已处理，返回首次结果。
///
/// # Errors
///
/// 失败时返回对应错误。
fn replay(current: &NodeRuntime, id: &EventId) -> Option<Result<Transition, TransitionError>> {
    let last_id = current.last_event_id.as_ref()?;
    if last_id == id {
        current.last_result.clone()
    } else {
        None
    }
}

/// 安装器与 agent 事件必须携带且等于当前会话。
///
/// # Errors
///
/// 失败时返回对应错误。
fn check_session(
    current: &NodeRuntime,
    session: Option<&SessionId>,
) -> Result<(), TransitionError> {
    match (current.session.as_ref(), session) {
        (Some(expected), Some(got)) if expected == got => Ok(()),
        (Some(_) | None, Some(_) | None) => Err(TransitionError::StaleSession),
    }
}

/// 按事件种类分派。
///
/// # Errors
///
/// 失败时返回对应错误。
fn dispatch(
    current: &NodeRuntime,
    envelope: &EventEnvelope,
    now: Timestamp,
    ttl: &Ttl,
) -> Result<Transition, TransitionError> {
    match &envelope.event {
        NodeEvent::Arm => arm(current, envelope, now, ttl),
        NodeEvent::Disarm => disarm(current, now),
        NodeEvent::Rearm { confirmation } => rearm(current, envelope, confirmation, now, ttl),
        NodeEvent::Reset => reset(current, now),
        NodeEvent::InstallerStarted => installer_started(current, now, ttl),
        NodeEvent::InstallerFinished => installer_finished(current, now, ttl),
        NodeEvent::InstallerFailed => installer_failed(current, now),
        NodeEvent::AgentStarted => agent_started(current, now, ttl),
        NodeEvent::AgentFinished => agent_finished(current, now),
        NodeEvent::AgentFailed => agent_failed(current, now),
        NodeEvent::DeadlineExpired { state, since } => {
            deadline_expired(current, *state, *since, now)
        }
    }
}

/// `Idle → Armed`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn arm(
    current: &NodeRuntime,
    envelope: &EventEnvelope,
    now: Timestamp,
    ttl: &Ttl,
) -> Result<Transition, TransitionError> {
    require_state(current, NodeState::Idle, NodeEventKind::Arm)?;
    require_new_session(envelope)?;
    Ok(enter(
        NodeState::Armed,
        now,
        ttl,
        vec![
            Effect::OpenSession,
            Effect::PublishDirectives,
            Effect::Audit(AuditKind::Armed),
        ],
    ))
}

/// `Armed → Idle`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn disarm(current: &NodeRuntime, now: Timestamp) -> Result<Transition, TransitionError> {
    require_state(current, NodeState::Armed, NodeEventKind::Disarm)?;
    Ok(leave_to_idle(now))
}

/// `Ready|Failed → Armed`，`Ready` 时校验确认串。
///
/// # Errors
///
/// 失败时返回对应错误。
fn rearm(
    current: &NodeRuntime,
    envelope: &EventEnvelope,
    confirmation: &str,
    now: Timestamp,
    ttl: &Ttl,
) -> Result<Transition, TransitionError> {
    match current.state {
        NodeState::Ready => {
            if confirmation != current.hostname.as_str() {
                return Err(TransitionError::BadConfirmation);
            }
        }
        NodeState::Failed => {}
        NodeState::Idle
        | NodeState::Armed
        | NodeState::Installing
        | NodeState::Installed
        | NodeState::Configuring => {
            return Err(not_allowed(current, NodeEventKind::Rearm));
        }
    }
    require_new_session(envelope)?;
    Ok(enter(
        NodeState::Armed,
        now,
        ttl,
        vec![
            Effect::OpenSession,
            Effect::PublishDirectives,
            Effect::Audit(AuditKind::Armed),
        ],
    ))
}

/// `Failed → Idle`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn reset(current: &NodeRuntime, now: Timestamp) -> Result<Transition, TransitionError> {
    require_state(current, NodeState::Failed, NodeEventKind::Reset)?;
    Ok(enter(
        NodeState::Idle,
        now,
        &Ttl::standard(),
        vec![Effect::Audit(AuditKind::Reset)],
    ))
}

/// `Armed → Installing`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn installer_started(
    current: &NodeRuntime,
    now: Timestamp,
    ttl: &Ttl,
) -> Result<Transition, TransitionError> {
    require_state(current, NodeState::Armed, NodeEventKind::InstallerStarted)?;
    Ok(enter(
        NodeState::Installing,
        now,
        ttl,
        vec![
            Effect::PublishDirectives,
            Effect::Audit(AuditKind::InstallStarted),
        ],
    ))
}

/// `Installing → Installed`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn installer_finished(
    current: &NodeRuntime,
    now: Timestamp,
    ttl: &Ttl,
) -> Result<Transition, TransitionError> {
    require_state(
        current,
        NodeState::Installing,
        NodeEventKind::InstallerFinished,
    )?;
    Ok(enter(
        NodeState::Installed,
        now,
        ttl,
        vec![Effect::Audit(AuditKind::InstallFinished)],
    ))
}

/// `Installing → Failed`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn installer_failed(current: &NodeRuntime, now: Timestamp) -> Result<Transition, TransitionError> {
    require_state(
        current,
        NodeState::Installing,
        NodeEventKind::InstallerFailed,
    )?;
    Ok(enter_failed(now, Outcome::Failure, AuditKind::Failed))
}

/// `Installed → Configuring`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn agent_started(
    current: &NodeRuntime,
    now: Timestamp,
    ttl: &Ttl,
) -> Result<Transition, TransitionError> {
    require_state(current, NodeState::Installed, NodeEventKind::AgentStarted)?;
    Ok(enter(
        NodeState::Configuring,
        now,
        ttl,
        vec![Effect::Audit(AuditKind::AgentStarted)],
    ))
}

/// `Configuring → Ready`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn agent_finished(current: &NodeRuntime, now: Timestamp) -> Result<Transition, TransitionError> {
    require_state(
        current,
        NodeState::Configuring,
        NodeEventKind::AgentFinished,
    )?;
    Ok(enter(
        NodeState::Ready,
        now,
        &Ttl::standard(),
        vec![
            Effect::CloseSession(Outcome::Success),
            Effect::EjectMedia,
            Effect::Audit(AuditKind::Ready),
        ],
    ))
}

/// `Configuring → Failed`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn agent_failed(current: &NodeRuntime, now: Timestamp) -> Result<Transition, TransitionError> {
    require_state(current, NodeState::Configuring, NodeEventKind::AgentFailed)?;
    Ok(enter_failed(now, Outcome::Failure, AuditKind::Failed))
}

/// 匹配当前状态与 `state_since` 的超时事件。
///
/// # Errors
///
/// 失败时返回对应错误。
fn deadline_expired(
    current: &NodeRuntime,
    state: NodeState,
    since: Timestamp,
    now: Timestamp,
) -> Result<Transition, TransitionError> {
    if current.state != state || current.state_since != since {
        return Err(TransitionError::StaleDeadline);
    }
    match current.state {
        NodeState::Armed => Ok(leave_to_idle(now)),
        NodeState::Installing | NodeState::Installed | NodeState::Configuring => {
            Ok(enter_failed(now, Outcome::Timeout, AuditKind::Failed))
        }
        NodeState::Idle | NodeState::Ready | NodeState::Failed => {
            Err(TransitionError::StaleDeadline)
        }
    }
}

/// 要求当前处于 `expected`。
///
/// # Errors
///
/// 失败时返回对应错误。
fn require_state(
    current: &NodeRuntime,
    expected: NodeState,
    event: NodeEventKind,
) -> Result<(), TransitionError> {
    if current.state == expected {
        Ok(())
    } else {
        Err(not_allowed(current, event))
    }
}

/// 武装类事件必须携带新会话。
///
/// # Errors
///
/// 失败时返回对应错误。
const fn require_new_session(envelope: &EventEnvelope) -> Result<(), TransitionError> {
    if envelope.session.is_some() {
        Ok(())
    } else {
        Err(TransitionError::MissingSession)
    }
}

/// 构造“不允许”错误。
const fn not_allowed(current: &NodeRuntime, event: NodeEventKind) -> TransitionError {
    TransitionError::NotAllowed {
        from: current.state,
        event,
    }
}

/// 进入带可选超时的新状态。
fn enter(to: NodeState, now: Timestamp, ttl: &Ttl, effects: Vec<Effect>) -> Transition {
    let deadline = ttl
        .for_state(to)
        .map(|duration| now.saturating_add(duration));
    Transition {
        to,
        deadline,
        effects,
    }
}

/// 回到 Idle 并关闭会话（解除武装或武装超时）。
fn leave_to_idle(_now: Timestamp) -> Transition {
    Transition {
        to: NodeState::Idle,
        deadline: None,
        effects: vec![
            Effect::CloseSession(Outcome::Cancelled),
            Effect::PublishDirectives,
            Effect::EjectMedia,
            Effect::Audit(AuditKind::Disarmed),
        ],
    }
}

/// 进入 Failed 并关闭会话。
fn enter_failed(_now: Timestamp, outcome: Outcome, audit: AuditKind) -> Transition {
    Transition {
        to: NodeState::Failed,
        deadline: None,
        effects: vec![
            Effect::CloseSession(outcome),
            Effect::PublishDirectives,
            Effect::EjectMedia,
            Effect::Audit(audit),
        ],
    }
}

#[cfg(test)]
mod tests;
