//! `redfish`：用 `xpxe-redfish` 对实验室 BMC（sushy-tools）执行探测与部署（Step 9）。
//!
//! - `redfish probe --url <https://…>`：输出证书指纹、主题、有效期与服务信息（不发凭据）。
//! - `redfish deploy --url … --user … --password-file … --pin <指纹> --image <ISO URL>
//!   [--profile <名>] [--system-id <ID>]`：资源发现 + 部署第 2–6 步，逐步输出事件。
//! - `redfish eject …`（参数同上，无 `--image`）。
//!
//! 失败时输出 `{"event":"error","code":…}` 并以非零退出；密码只从文件读取，从不输出。

use std::time::Duration;

use serde_json::json;
use xpxe_redfish::{
    BmcConfig, Credentials, Endpoint, Fingerprint, ProfileId, RedfishError, Session, Timing, probe,
};

use crate::common::{Args, LabError, LabResult, emit, runtime};

impl From<RedfishError> for LabError {
    fn from(value: RedfishError) -> Self {
        Self(value.to_string())
    }
}

/// 子命令入口。
///
/// # Errors
///
/// 参数缺失或 Redfish 操作失败时返回错误。
pub(crate) fn run(args: &Args) -> LabResult {
    let action = args.first().unwrap_or_default();
    let outcome = runtime()?.block_on(async {
        match action.as_str() {
            "probe" => run_probe(args).await,
            "deploy" | "eject" => run_session(args, &action).await,
            other => Err(RedfishError::new(
                "usage",
                format!("用法：xpxe-lab redfish <probe|deploy|eject>，收到 {other:?}"),
            )),
        }
    });
    if let Err(error) = &outcome {
        emit(&json!({"event": "error", "code": error.code(), "message": error.message()}))?;
    }
    Ok(outcome?)
}

/// 必填参数。
///
/// # Errors
///
/// 缺失时返回 `usage`。
fn need(args: &Args, key: &str) -> Result<String, RedfishError> {
    args.value(key)
        .ok_or_else(|| RedfishError::new("usage", format!("缺少参数 {key}")))
}

/// `probe`。
///
/// # Errors
///
/// 探测失败时返回错误。
async fn run_probe(args: &Args) -> Result<(), RedfishError> {
    let endpoint = Endpoint::parse(&need(args, "--url")?)?;
    let report = probe(&endpoint, Duration::from_secs(15)).await?;
    emit(&json!({
        "event": "probe",
        "fingerprint": report.fingerprint.to_string(),
        "subject": report.certificate.subject,
        "issuer": report.certificate.issuer,
        "not_before": report.certificate.not_before,
        "not_after": report.certificate.not_after,
        "vendor": report.vendor,
        "redfish_version": report.redfish_version,
        "profile": report.suggested_profile.as_str(),
    }))
    .map_err(|error| RedfishError::new("io", error.0))
}

/// 由参数组装连接配置。
///
/// # Errors
///
/// 参数缺失或不合法时返回错误。
fn config(args: &Args) -> Result<BmcConfig, RedfishError> {
    let password = std::fs::read_to_string(need(args, "--password-file")?)
        .map_err(|error| RedfishError::new("usage", format!("密码文件不可读：{error}")))?;
    let profile = match args.value("--profile") {
        Some(name) => Some(
            ProfileId::from_name(&name)
                .ok_or_else(|| RedfishError::new("usage", format!("未知画像 {name}")))?,
        ),
        None => None,
    };
    Ok(BmcConfig {
        endpoint: Endpoint::parse(&need(args, "--url")?)?,
        credentials: Credentials::new(need(args, "--user")?, password.trim().to_owned()),
        pin: Fingerprint::parse(&need(args, "--pin")?)?,
        profile,
        system_id: args.value("--system-id"),
        timing: Timing::default(),
    })
}

/// `deploy` / `eject`。
///
/// # Errors
///
/// 连接、发现或部署失败时返回错误。
async fn run_session(args: &Args, action: &str) -> Result<(), RedfishError> {
    let io = |error: LabError| RedfishError::new("io", error.0);
    let session = Session::open(&config(args)?).await?;
    let target = session.discover().await?;
    emit(&json!({
        "event": "target",
        "profile": session.profile_id().as_str(),
        "vendor": session.vendor(),
        "token_session": session.uses_token(),
        "system": target.system,
        "vmedia": target.vmedia,
        "boot_mode": target.boot_mode,
        "power_state": target.power_state,
    }))
    .map_err(io)?;
    let outcome = if action == "eject" {
        session.eject(&target).await
    } else {
        let image = need(args, "--image")?;
        let mut events = Vec::new();
        let result = session.deploy(&target, &image, &mut events).await;
        for event in &events {
            emit(&json!({"event": "step", "step": event.step, "ok": event.ok, "detail": event.detail}))
                .map_err(io)?;
        }
        result
    };
    session.close().await?;
    outcome?;
    emit(&json!({"event": action, "ok": true})).map_err(io)
}
