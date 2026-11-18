//! 探测、会话、资源发现与虚拟介质部署流程。

use std::time::Duration;

use http::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::pin::Fingerprint;
use crate::profile::{ProfileId, ResetType, VmediaLocation};
use crate::settings::{BmcConfig, Endpoint, Timing};
use crate::transport::{Auth, Reply, Transport};
use crate::wire::{
    action_target, boot_body, cd_allowed, extended_info, insert_body, is_cd, link, members, text,
};
use crate::x509::{CertSummary, summarize};
use crate::{RedfishError, RedfishResult};

/// `ServiceRoot` 路径。
const ROOT: &str = "/redfish/v1";

/// `probe` 结果：给管理员核对的证书信息与服务信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// 终端证书指纹。
    pub fingerprint: Fingerprint,
    /// 证书主题、颁发者与有效期（解析失败时为空）。
    pub certificate: CertSummary,
    /// `ServiceRoot.Vendor`。
    pub vendor: Option<String>,
    /// `ServiceRoot.RedfishVersion`。
    pub redfish_version: Option<String>,
    /// 按 Vendor 建议的画像。
    pub suggested_profile: ProfileId,
}

/// 接受任意证书完成握手，只请求无需认证的 `GET /redfish/v1`；不发送任何凭据。
///
/// # Errors
///
/// 连接失败、超时或 `ServiceRoot` 不可读时返回错误。
pub async fn probe(endpoint: &Endpoint, timeout: Duration) -> RedfishResult<ProbeReport> {
    let transport = Transport::new(endpoint, None, timeout)?;
    let root = transport
        .send(Method::GET, ROOT, &Auth::Anonymous, None)
        .await?
        .success("读取 ServiceRoot")?
        .body;
    let der = transport
        .seen_der()
        .ok_or_else(|| RedfishError::new("transport", "握手中未看到证书".to_owned()))?;
    let vendor = text(&root, "Vendor").map(ToOwned::to_owned);
    Ok(ProbeReport {
        fingerprint: Fingerprint::of_der(&der),
        certificate: summarize(&der).unwrap_or_default(),
        suggested_profile: ProfileId::from_vendor(vendor.as_deref()),
        vendor,
        redfish_version: text(&root, "RedfishVersion").map(ToOwned::to_owned),
    })
}

/// 部署目标：系统与 CD 虚拟介质资源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `ComputerSystem` 路径。
    pub system: String,
    /// CD/DVD `VirtualMedia` 路径。
    pub vmedia: String,
    /// 发现时的 `Boot.BootSourceOverrideMode`。
    pub boot_mode: Option<String>,
    /// 发现时的 `PowerState`。
    pub power_state: Option<String>,
}

/// 部署流程中的一步（写节点事件与审计）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployEvent {
    /// 步骤：`eject`、`insert`、`inserted`、`boot_override`、`reset`。
    pub step: &'static str,
    /// 是否成功（`eject` 失败只告警）。
    pub ok: bool,
    /// 说明。
    pub detail: String,
}

/// 电源操作（Web 上需二次确认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    /// 开机。
    On,
    /// 强制关机。
    ForceOff,
    /// 强制重启。
    ForceRestart,
}

/// 记录一步。
fn record(events: &mut Vec<DeployEvent>, step: &'static str, ok: bool, detail: String) {
    events.push(DeployEvent { step, ok, detail });
}

/// 截止时间。
///
/// # Errors
///
/// 时长溢出时返回 `timeout`。
fn deadline(after: Duration) -> RedfishResult<Instant> {
    Instant::now()
        .checked_add(after)
        .ok_or_else(|| RedfishError::new("timeout", "等待时长溢出".to_owned()))
}

/// 已认证的 BMC 会话（证书已固定）。
#[derive(Debug)]
pub struct Session {
    /// 传输。
    transport: Transport,
    /// 认证方式。
    auth: Auth,
    /// 会话资源（令牌模式，结束时删除）。
    logout_uri: Option<String>,
    /// 生效的画像。
    profile_id: ProfileId,
    /// `ServiceRoot.Vendor`。
    vendor: Option<String>,
    /// 多系统时使用的系统 ID。
    system_id: Option<String>,
    /// 轮询时序。
    timing: Timing,
}

impl Session {
    /// 连接 BMC（只接受固定指纹的证书）并登录：优先 `SessionService` 令牌，不支持时 Basic。
    ///
    /// # Errors
    ///
    /// 证书变化（`cert_changed`）、凭据错误（`unauthorized`）、连接失败或超时时返回错误。
    pub async fn open(config: &BmcConfig) -> RedfishResult<Self> {
        let transport = Transport::new(&config.endpoint, Some(config.pin), config.timing.request)?;
        let root = transport
            .send(Method::GET, ROOT, &Auth::Anonymous, None)
            .await?
            .success("读取 ServiceRoot")?
            .body;
        let vendor = text(&root, "Vendor").map(ToOwned::to_owned);
        let (auth, logout_uri) = login(&transport, &root, config).await?;
        Ok(Self {
            transport,
            auth,
            logout_uri,
            profile_id: config
                .profile
                .unwrap_or_else(|| ProfileId::from_vendor(vendor.as_deref())),
            vendor,
            system_id: config.system_id.clone(),
            timing: config.timing,
        })
    }

    /// 生效的画像。
    #[must_use]
    pub const fn profile_id(&self) -> ProfileId {
        self.profile_id
    }

    /// `ServiceRoot.Vendor`。
    #[must_use]
    pub fn vendor(&self) -> Option<&str> {
        self.vendor.as_deref()
    }

    /// 是否使用会话令牌（否则 Basic）。
    #[must_use]
    pub const fn uses_token(&self) -> bool {
        matches!(self.auth, Auth::Token(_))
    }

    /// 结束会话（令牌模式下 `DELETE` 会话资源）。
    ///
    /// # Errors
    ///
    /// 删除请求失败时返回错误（404 视为已结束）。
    pub async fn close(self) -> RedfishResult<()> {
        if let (Some(uri), Auth::Token(_)) = (&self.logout_uri, &self.auth) {
            let reply = self.send(Method::DELETE, uri, None).await?;
            if reply.status != StatusCode::NOT_FOUND {
                let _done = reply.success("删除会话")?;
            }
        }
        Ok(())
    }

    /// 带认证发送。
    ///
    /// # Errors
    ///
    /// 传输失败时返回错误。
    async fn send(&self, method: Method, path: &str, body: Option<&Value>) -> RedfishResult<Reply> {
        self.transport.send(method, path, &self.auth, body).await
    }

    /// `GET` 并要求 2xx。
    ///
    /// # Errors
    ///
    /// 传输失败或非 2xx 时返回错误。
    async fn get(&self, path: &str) -> RedfishResult<Value> {
        Ok(self
            .send(Method::GET, path, None)
            .await?
            .success(&format!("GET {path}"))?
            .body)
    }

    /// 发现系统与 CD 虚拟介质，并确认支持一次性 `Cd` 启动。
    ///
    /// # Errors
    ///
    /// 找不到/无法确定系统（`no_system`、`system_ambiguous`）、没有 CD 介质（`no_virtual_media`）、
    /// 不允许 `Cd` 启动（`cd_not_supported`）或请求失败时返回错误。
    pub async fn discover(&self) -> RedfishResult<Target> {
        let root = self.get(ROOT).await?;
        let system = self.find_system(&root).await?;
        let body = self.get(&system).await?;
        if !cd_allowed(&body) {
            return Err(RedfishError::new(
                "cd_not_supported",
                format!("{system} 的 BootSourceOverrideTarget 不允许 Cd"),
            ));
        }
        let vmedia = self.find_vmedia(&root, &body).await?.ok_or_else(|| {
            RedfishError::new("no_virtual_media", "未找到 CD/DVD 虚拟介质".to_owned())
        })?;
        let boot = body.get("Boot").cloned().unwrap_or(Value::Null);
        Ok(Target {
            system,
            vmedia,
            boot_mode: text(&boot, "BootSourceOverrideMode").map(ToOwned::to_owned),
            power_state: text(&body, "PowerState").map(ToOwned::to_owned),
        })
    }

    /// `Systems` 集合：唯一成员直接用；多个时按 `system_id` 匹配路径末段。
    ///
    /// # Errors
    ///
    /// 集合缺失、为空或无法确定成员时返回错误。
    async fn find_system(&self, root: &Value) -> RedfishResult<String> {
        let path = link(root, "Systems").ok_or_else(|| {
            RedfishError::new("no_system", "ServiceRoot 中没有 Systems".to_owned())
        })?;
        let all = members(&self.get(&path).await?);
        if let [only] = all.as_slice() {
            return Ok(only.clone());
        }
        let wanted = self.system_id.as_deref().ok_or_else(|| {
            RedfishError::new(
                "system_ambiguous",
                format!(
                    "BMC 有 {} 个系统，需要在节点规格中指定 system_id",
                    all.len()
                ),
            )
        })?;
        all.into_iter()
            .find(|member| member.trim_end_matches('/').rsplit('/').next() == Some(wanted))
            .ok_or_else(|| RedfishError::new("no_system", format!("未找到系统 {wanted}")))
    }

    /// 按画像定位 CD 虚拟介质。
    ///
    /// # Errors
    ///
    /// 请求失败时返回错误。
    async fn find_vmedia(&self, root: &Value, system: &Value) -> RedfishResult<Option<String>> {
        let location = self.profile_id.profile().vmedia;
        if location != VmediaLocation::Manager
            && let Some(found) = self.cd_in_opt(link(system, "VirtualMedia")).await?
        {
            return Ok(Some(found));
        }
        if location == VmediaLocation::System {
            return Ok(None);
        }
        let Some(managers) = link(root, "Managers") else {
            return Ok(None);
        };
        for manager in members(&self.get(&managers).await?) {
            let collection = link(&self.get(&manager).await?, "VirtualMedia");
            if let Some(found) = self.cd_in_opt(collection).await? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// 可选集合版本的 [`Session::cd_in`]。
    ///
    /// # Errors
    ///
    /// 请求失败时返回错误。
    async fn cd_in_opt(&self, collection: Option<String>) -> RedfishResult<Option<String>> {
        match collection {
            Some(path) => self.cd_in(&path).await,
            None => Ok(None),
        }
    }

    /// 集合中第一个 `MediaTypes` 含 CD/DVD 的成员。
    ///
    /// # Errors
    ///
    /// 请求失败时返回错误。
    async fn cd_in(&self, collection: &str) -> RedfishResult<Option<String>> {
        for member in members(&self.get(collection).await?) {
            if is_cd(&self.get(&member).await?) {
                return Ok(Some(member));
            }
        }
        Ok(None)
    }

    /// 部署第 2–6 步：（按需）弹出、插入 ISO、等待 `Inserted`、一次性 UEFI CD 启动、重启。
    ///
    /// # Errors
    ///
    /// 插入失败（附 `ExtendedInfo`）、`Inserted` 超时、当前为 Legacy 模式（`legacy_boot_mode`）、
    /// 重启失败或证书变化时返回错误；弹出失败只记录事件。
    pub async fn deploy(
        &self,
        target: &Target,
        image_url: &str,
        events: &mut Vec<DeployEvent>,
    ) -> RedfishResult<()> {
        self.eject_if_needed(target, events).await;
        self.insert(target, image_url, events).await?;
        self.wait_inserted(target).await?;
        record(
            events,
            "inserted",
            true,
            format!("{} Inserted=true", target.vmedia),
        );
        self.set_boot_once(target, events).await?;
        let reset = self.reset_for_boot(target).await?;
        record(
            events,
            "reset",
            true,
            format!("ResetType={}", reset.as_str()),
        );
        Ok(())
    }

    /// 已插入介质（或画像要求）时先弹出；失败只记录。
    async fn eject_if_needed(&self, target: &Target, events: &mut Vec<DeployEvent>) {
        let inserted = match self.get(&target.vmedia).await {
            Ok(media) => media.get("Inserted").and_then(Value::as_bool) == Some(true),
            Err(error) => {
                record(events, "eject", false, error.to_string());
                return;
            }
        };
        if !inserted && !self.profile_id.profile().eject_before_insert {
            return;
        }
        match self.eject(target).await {
            Ok(()) => record(events, "eject", true, format!("{} 已弹出", target.vmedia)),
            Err(error) => record(events, "eject", false, error.to_string()),
        }
    }

    /// `InsertMedia`（字段按画像裁剪；202 + `TaskMonitor` 时轮询任务）。
    ///
    /// # Errors
    ///
    /// BMC 拒绝或任务失败时返回错误。
    async fn insert(
        &self,
        target: &Target,
        image_url: &str,
        events: &mut Vec<DeployEvent>,
    ) -> RedfishResult<()> {
        let media = self.get(&target.vmedia).await?;
        let action = action_target(&media, &target.vmedia, "VirtualMedia.InsertMedia");
        let profile = self.profile_id.profile();
        let body = insert_body(image_url, profile.insert_fields);
        let reply = self.send(Method::POST, &action, Some(&body)).await?;
        if let (StatusCode::ACCEPTED, true, Some(monitor)) =
            (reply.status, profile.task_polling, &reply.location)
        {
            self.wait_task(monitor).await?;
        }
        let _done = reply.success("InsertMedia")?;
        record(
            events,
            "insert",
            true,
            format!("{image_url} → {}", target.vmedia),
        );
        Ok(())
    }

    /// 轮询直至 `Inserted = true`。
    ///
    /// # Errors
    ///
    /// 超过 [`Timing::insert`] 时返回 `timeout`。
    async fn wait_inserted(&self, target: &Target) -> RedfishResult<()> {
        let until = deadline(self.timing.insert)?;
        loop {
            let media = self.get(&target.vmedia).await?;
            if media.get("Inserted").and_then(Value::as_bool) == Some(true) {
                return Ok(());
            }
            if Instant::now() >= until {
                return Err(RedfishError::new(
                    "timeout",
                    format!(
                        "{} 在 {:?} 内未报告 Inserted=true",
                        target.vmedia, self.timing.insert
                    ),
                ));
            }
            tokio::time::sleep(self.timing.poll).await;
        }
    }

    /// 轮询 202 任务直至结束。
    ///
    /// # Errors
    ///
    /// 任务失败或超时时返回错误。
    async fn wait_task(&self, monitor: &str) -> RedfishResult<()> {
        let until = deadline(self.timing.task)?;
        loop {
            let reply = self.send(Method::GET, monitor, None).await?;
            let state = text(&reply.body, "TaskState").unwrap_or_default();
            let running = matches!(state, "New" | "Running" | "Pending" | "Starting");
            if reply.status != StatusCode::ACCEPTED && !running {
                return task_outcome(monitor, reply);
            }
            if Instant::now() >= until {
                return Err(RedfishError::new(
                    "timeout",
                    format!("任务 {monitor} 未在时限内结束"),
                ));
            }
            tokio::time::sleep(self.timing.poll).await;
        }
    }

    /// 一次性 UEFI CD 启动；BMC 不接受 `BootSourceOverrideMode`（400）时省略该字段，并要求当前模式不是 Legacy。
    ///
    /// # Errors
    ///
    /// BMC 拒绝或当前为 Legacy（`legacy_boot_mode`）时返回错误。
    async fn set_boot_once(
        &self,
        target: &Target,
        events: &mut Vec<DeployEvent>,
    ) -> RedfishResult<()> {
        let first = self
            .send(Method::PATCH, &target.system, Some(&boot_body(true)))
            .await?;
        if first.status.is_success() {
            record(events, "boot_override", true, "Once/Cd/UEFI".to_owned());
            return Ok(());
        }
        if first.status != StatusCode::BAD_REQUEST {
            return first.success("设置一次性 CD 启动").map(|_reply| ());
        }
        let refused = extended_info(&first.body);
        let _second = self
            .send(Method::PATCH, &target.system, Some(&boot_body(false)))
            .await?
            .success("设置一次性 CD 启动（省略模式）")?;
        let system = self.get(&target.system).await?;
        let mode = system
            .get("Boot")
            .and_then(|boot| text(boot, "BootSourceOverrideMode"))
            .unwrap_or_default();
        if mode == "Legacy" {
            return Err(RedfishError::new(
                "legacy_boot_mode",
                "BMC 报告当前启动模式为 Legacy：该服务器需切换到 UEFI 或改用局域网 PXE".to_owned(),
            ));
        }
        record(
            events,
            "boot_override",
            true,
            format!("Once/Cd（BMC 不接受 BootSourceOverrideMode：{refused}；当前模式 {mode}）"),
        );
        Ok(())
    }

    /// 按电源状态与画像重启。
    ///
    /// # Errors
    ///
    /// 请求失败时返回错误。
    async fn reset_for_boot(&self, target: &Target) -> RedfishResult<ResetType> {
        let system = self.get(&target.system).await?;
        let policy = self.profile_id.profile().reset;
        let reset = if text(&system, "PowerState") == Some("On") {
            policy.when_on
        } else {
            policy.when_off
        };
        self.reset(&target.system, &system, reset).await?;
        Ok(reset)
    }

    /// `ComputerSystem.Reset`。
    ///
    /// # Errors
    ///
    /// 请求失败时返回错误。
    async fn reset(&self, path: &str, system: &Value, reset: ResetType) -> RedfishResult<()> {
        let action = action_target(system, path, "ComputerSystem.Reset");
        let body = json!({ "ResetType": reset.as_str() });
        self.send(Method::POST, &action, Some(&body))
            .await?
            .success("ComputerSystem.Reset")
            .map(|_reply| ())
    }

    /// `EjectMedia`。
    ///
    /// # Errors
    ///
    /// 请求失败时返回错误。
    pub async fn eject(&self, target: &Target) -> RedfishResult<()> {
        let media = self.get(&target.vmedia).await?;
        let action = action_target(&media, &target.vmedia, "VirtualMedia.EjectMedia");
        self.send(Method::POST, &action, Some(&json!({})))
            .await?
            .success("EjectMedia")
            .map(|_reply| ())
    }

    /// 电源操作。
    ///
    /// # Errors
    ///
    /// 请求失败时返回错误。
    pub async fn power(&self, target: &Target, action: PowerAction) -> RedfishResult<()> {
        let system = self.get(&target.system).await?;
        let reset = match action {
            PowerAction::On => ResetType::On,
            PowerAction::ForceOff => ResetType::ForceOff,
            PowerAction::ForceRestart => ResetType::ForceRestart,
        };
        self.reset(&target.system, &system, reset).await
    }
}

/// 已结束任务的结果：`Exception`/`Killed`/`Cancelled` 为失败。
///
/// # Errors
///
/// 任务失败或 HTTP 非 2xx 时返回错误。
fn task_outcome(monitor: &str, reply: Reply) -> RedfishResult<()> {
    let state = text(&reply.body, "TaskState").unwrap_or_default();
    if matches!(state, "Exception" | "Killed" | "Cancelled") {
        return Err(RedfishError::new(
            "task_failed",
            format!(
                "任务 {monitor} 状态 {state}：{}",
                extended_info(&reply.body)
            ),
        ));
    }
    reply.success("任务").map(|_reply| ())
}

/// 登录：有 `SessionService` 时创建会话取令牌；404/405/501 或没有令牌时回落 Basic。
///
/// # Errors
///
/// 凭据错误（`unauthorized`）或请求失败时返回错误。
async fn login(
    transport: &Transport,
    root: &Value,
    config: &BmcConfig,
) -> RedfishResult<(Auth, Option<String>)> {
    let basic = Auth::basic(&config.credentials);
    let sessions = root
        .get("Links")
        .and_then(|links| link(links, "Sessions"))
        .or_else(|| {
            link(root, "SessionService").map(|_service| format!("{ROOT}/SessionService/Sessions"))
        });
    let Some(path) = sessions else {
        return Ok((basic, None));
    };
    let body = json!({
        "UserName": config.credentials.username(),
        "Password": config.credentials.password(),
    });
    let reply = transport
        .send(Method::POST, &path, &Auth::Anonymous, Some(&body))
        .await?;
    if matches!(
        reply.status,
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
    ) {
        return Ok((basic, None));
    }
    let created = reply.success("创建 Redfish 会话")?;
    Ok(match created.token {
        Some(token) => (Auth::Token(token), created.location),
        None => (basic, None),
    })
}
