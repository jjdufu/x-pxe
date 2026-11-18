//! 画像选择与名称往返。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{ProfileId, TransferFields, VmediaLocation};

/// 按 Vendor 自动选择；未知与缺失都回落 Generic；名称可往返。
///
/// # Errors
///
/// 选择不符时返回错误。
#[test]
fn vendor_selection() -> TestResult {
    ensure_eq(
        &ProfileId::from_vendor(Some("Dell")),
        &ProfileId::DellIdrac9,
        "Dell",
    )?;
    ensure_eq(
        &ProfileId::from_vendor(Some("HPE")),
        &ProfileId::HpeIlo5,
        "HPE",
    )?;
    ensure_eq(
        &ProfileId::from_vendor(Some("Supermicro")),
        &ProfileId::SupermicroX12,
        "Supermicro",
    )?;
    ensure_eq(
        &ProfileId::from_vendor(Some("Lenovo")),
        &ProfileId::LenovoXcc,
        "Lenovo",
    )?;
    ensure_eq(&ProfileId::from_vendor(None), &ProfileId::Generic, "缺失")?;
    ensure_eq(
        &ProfileId::from_vendor(Some("Acme")),
        &ProfileId::Generic,
        "未知",
    )?;
    for id in ProfileId::ALL {
        ensure_eq(&ProfileId::from_name(id.as_str()), &Some(id), "名称往返")?;
    }
    ensure(
        ProfileId::Generic.verified() && !ProfileId::HpeIlo6.verified(),
        "验证标注",
    )?;
    Ok(())
}

/// 关键差异：HPE 在 Manager 下且不带传输字段；Generic 自动定位且不强制弹出。
///
/// # Errors
///
/// 画像内容不符时返回错误。
#[test]
fn profile_differences() -> TestResult {
    let hpe = ProfileId::HpeIlo5.profile();
    ensure_eq(&hpe.vmedia, &VmediaLocation::Manager, "HPE 位置")?;
    ensure_eq(
        &hpe.insert_fields.transfer,
        &TransferFields::Omit,
        "HPE 字段",
    )?;
    let generic = ProfileId::Generic.profile();
    ensure_eq(&generic.vmedia, &VmediaLocation::Auto, "Generic 位置")?;
    ensure(!generic.eject_before_insert, "Generic 不强制弹出")?;
    ensure_eq(
        &ProfileId::LenovoXcc.profile().insert_fields.transfer,
        &TransferFields::MethodAndProtocol,
        "Lenovo 字段",
    )?;
    Ok(())
}
