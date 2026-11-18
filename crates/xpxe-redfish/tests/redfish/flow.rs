//! 部署流程：按厂商画像的资源布局与差异。

use serde_json::{Value, json};
use xpxe_redfish::{DeployEvent, RedfishResult, Session};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use crate::bmc::{Quirk, Shared, find, mock};
use crate::server::{config, runtime, start};

/// ISO 地址。
const IMAGE: &str = "http://10.77.0.1:8080/p/tok/boot.iso";

/// 一次部署的结果：模拟状态、事件与部署结果。
type Run = (Shared, Vec<DeployEvent>, RedfishResult<()>);

/// 启动模拟 BMC 并跑一次完整部署，返回状态、事件与结果。
///
/// # Errors
///
/// 服务启动或会话建立失败时返回错误（部署本身的失败放在结果里）。
fn deploy(vendor: &str, quirks: &[Quirk]) -> TestResult<Run> {
    runtime()?.block_on(async {
        let (state, handler) = mock(vendor, quirks);
        let port = start("bmc", handler).await?;
        let mut cfg = config(port, "bmc")?;
        cfg.timing.insert = std::time::Duration::from_millis(300);
        let session = Session::open(&cfg).await?;
        let target = session.discover().await?;
        let mut events = Vec::new();
        let outcome = session.deploy(&target, IMAGE, &mut events).await;
        session.close().await?;
        Ok((state, events, outcome))
    })
}

/// 事件步骤序列。
fn steps(events: &[DeployEvent]) -> Vec<&'static str> {
    events.iter().map(|event| event.step).collect()
}

/// 唯一一次 `InsertMedia` 的请求体。
///
/// # Errors
///
/// 结果不符时返回错误。
fn insert_body(state: &Shared) -> TestResult<Value> {
    let all = find(state, "POST", "VirtualMedia.InsertMedia");
    Ok(all.first().ok_or("没有 InsertMedia")?.body.clone())
}

/// 唯一（最后）一次重启类型。
///
/// # Errors
///
/// 结果不符时返回错误。
fn reset_type(state: &Shared) -> TestResult<Value> {
    let all = find(state, "POST", "ComputerSystem.Reset");
    Ok(all
        .last()
        .ok_or("没有 Reset")?
        .body
        .get("ResetType")
        .cloned()
        .unwrap_or(Value::Null))
}

/// Generic（sushy-tools 布局）：System 下的介质、不带传输字段、UEFI 一次性 CD、已开机 `ForceRestart`。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn generic_system_layout() -> TestResult {
    let (state, events, outcome) = deploy("", &[])?;
    outcome?;
    ensure_eq(
        &steps(&events),
        &vec!["insert", "inserted", "boot_override", "reset"],
        "步骤",
    )?;
    let body = insert_body(&state)?;
    ensure_eq(
        &body,
        &json!({"Image": IMAGE, "Inserted": true, "WriteProtected": true}),
        "InsertMedia 字段",
    )?;
    let patch = find(&state, "PATCH", "/Systems/1");
    ensure_eq(
        &patch.first().map(|item| item.body.clone()),
        &Some(
            json!({"Boot": {"BootSourceOverrideTarget": "Cd", "BootSourceOverrideEnabled": "Once", "BootSourceOverrideMode": "UEFI"}}),
        ),
        "PATCH Boot",
    )?;
    ensure_eq(
        &reset_type(&state)?,
        &json!("ForceRestart"),
        "已开机强制重启",
    )?;
    Ok(())
}

/// HPE：Manager 下的介质、已插入时必须先弹出、不带传输字段。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn hpe_manager_layout_ejects_first() -> TestResult {
    let (state, events, outcome) = deploy("HPE", &[Quirk::ManagerVmedia, Quirk::MustEject])?;
    outcome?;
    ensure_eq(
        &steps(&events),
        &vec!["eject", "insert", "inserted", "boot_override", "reset"],
        "先弹出",
    )?;
    ensure(events.first().is_some_and(|event| event.ok), "弹出成功")?;
    ensure(
        find(&state, "POST", "VirtualMedia.InsertMedia")
            .iter()
            .all(|item| item.path.starts_with("/redfish/v1/Managers/1/")),
        "Manager 下插入",
    )?;
    ensure(
        insert_body(&state)?.get("TransferProtocolType").is_none(),
        "iLO 不带传输字段",
    )?;
    Ok(())
}

/// Dell：202 + `TaskMonitor` 轮询到完成，携带 `TransferProtocolType`。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn dell_task_polling() -> TestResult {
    let (state, _events, outcome) = deploy("Dell Inc.", &[Quirk::Task202])?;
    outcome?;
    ensure_eq(
        &insert_body(&state)?.get("TransferProtocolType"),
        &Some(&json!("HTTP")),
        "传输协议",
    )?;
    let polls = state.lock().map_err(|_poisoned| "锁中毒")?.task_polls;
    ensure(polls >= 2, "轮询任务直至完成")?;
    Ok(())
}

/// Supermicro：Manager 下的介质、已关机时 `On`；Lenovo：`TransferMethod: Stream`。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn supermicro_off_and_lenovo_fields() -> TestResult {
    let (state, _events, outcome) =
        deploy("Supermicro", &[Quirk::ManagerVmedia, Quirk::PoweredOff])?;
    outcome?;
    ensure_eq(&reset_type(&state)?, &json!("On"), "已关机开机")?;
    let (lenovo, _, done) = deploy("Lenovo", &[Quirk::ManagerVmedia])?;
    done?;
    ensure_eq(
        &insert_body(&lenovo)?.get("TransferMethod"),
        &Some(&json!("Stream")),
        "Stream",
    )?;
    Ok(())
}

/// `ExtendedInfo` 错误透传；插入失败后不设置启动、不重启。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn insert_error_extended_info() -> TestResult {
    let (state, _events, outcome) = deploy("", &[Quirk::InsertError])?;
    let error = outcome.err().ok_or("应失败")?;
    ensure_eq(&error.code(), &"http_status", "错误码")?;
    ensure(
        error.message().contains("Image URL is not reachable."),
        "ExtendedInfo",
    )?;
    ensure(
        find(&state, "PATCH", "").is_empty() && find(&state, "POST", "Reset").is_empty(),
        "未继续",
    )?;
    Ok(())
}

/// `Inserted` 延迟两次读取后变为 true 时成功；一直不变时超时。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn inserted_delay_and_timeout() -> TestResult {
    let (_state, _events, outcome) = deploy("", &[Quirk::InsertDelay(2)])?;
    outcome?;
    let (state, _, slow) = deploy("", &[Quirk::InsertDelay(1000)])?;
    ensure_eq(
        &slow.err().map(|error| error.code()),
        &Some("timeout"),
        "超时",
    )?;
    ensure(find(&state, "PATCH", "").is_empty(), "超时后不设置启动")?;
    Ok(())
}

/// BMC 不接受 `BootSourceOverrideMode`：省略后重试；当前模式 Legacy 时失败并提示改用 UEFI/PXE。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn boot_mode_fallback_and_legacy() -> TestResult {
    let (state, events, outcome) = deploy("", &[Quirk::RejectBootMode])?;
    outcome?;
    let patches = find(&state, "PATCH", "/Systems/1");
    ensure_eq(&patches.len(), &2, "两次 PATCH")?;
    ensure(
        patches
            .last()
            .is_some_and(|item| item.body.pointer("/Boot/BootSourceOverrideMode").is_none()),
        "第二次省略模式",
    )?;
    ensure(
        events
            .iter()
            .any(|event| event.step == "boot_override" && event.detail.contains("不接受")),
        "事件说明",
    )?;
    let (legacy, _, failed) = deploy("", &[Quirk::RejectBootMode, Quirk::Legacy])?;
    let error = failed.err().ok_or("Legacy 应失败")?;
    ensure_eq(&error.code(), &"legacy_boot_mode", "错误码")?;
    ensure(error.message().contains("局域网 PXE"), "提示")?;
    ensure(find(&legacy, "POST", "Reset").is_empty(), "不重启")?;
    Ok(())
}
