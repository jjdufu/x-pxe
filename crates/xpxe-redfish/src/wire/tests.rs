//! JSON 辅助测试。

use serde_json::json;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{action_target, boot_body, cd_allowed, extended_info, insert_body};
use crate::profile::{InsertFields, TransferFields};

/// `ExtendedInfo` 汇总 `error.message` 与消息 ID，去重。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn extended_info_messages() -> TestResult {
    let body = json!({"error": {"code": "Base.1.0.GeneralError", "message": "A general error has occurred.",
        "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.PropertyValueNotInList", "Message": "Value Floppy not allowed."}]}});
    ensure_eq(
        &extended_info(&body).as_str(),
        &"A general error has occurred.；Value Floppy not allowed.（Base.1.8.PropertyValueNotInList）",
        "汇总",
    )?;
    ensure_eq(&extended_info(&json!(null)).as_str(), &"", "空")?;
    Ok(())
}

/// 请求体按画像裁剪；PATCH 可省略模式字段；动作目标回落默认路径。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn bodies_and_targets() -> TestResult {
    let omit = insert_body(
        "http://x/boot.iso",
        InsertFields {
            transfer: TransferFields::Omit,
            write_protected: true,
        },
    );
    ensure(
        omit.get("TransferMethod").is_none() && omit.get("TransferProtocolType").is_none(),
        "Omit",
    )?;
    let full = insert_body(
        "http://x/boot.iso",
        InsertFields {
            transfer: TransferFields::MethodAndProtocol,
            write_protected: false,
        },
    );
    ensure_eq(
        &full.get("TransferMethod"),
        &Some(&json!("Stream")),
        "Stream",
    )?;
    ensure(full.get("WriteProtected").is_none(), "不带写保护")?;
    ensure(
        boot_body(false)
            .pointer("/Boot/BootSourceOverrideMode")
            .is_none(),
        "省略模式",
    )?;
    ensure_eq(
        &boot_body(true).pointer("/Boot/BootSourceOverrideMode"),
        &Some(&json!("UEFI")),
        "UEFI",
    )?;
    ensure_eq(
        &action_target(&json!({}), "/redfish/v1/Systems/1/", "ComputerSystem.Reset").as_str(),
        &"/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
        "默认目标",
    )?;
    ensure(cd_allowed(&json!({})), "未列出视为允许")?;
    ensure(
        !cd_allowed(
            &json!({"Boot": {"BootSourceOverrideTarget@Redfish.AllowableValues": ["Pxe"]}}),
        ),
        "不含 Cd",
    )?;
    Ok(())
}
