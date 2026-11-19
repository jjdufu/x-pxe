//! ISO 元数据、抽取产物与分块上传记录。

use serde_json::json;
use xpxe_store::{ExtractKey, ExtractRecord, NewIso, UploadRecord};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{at, open};

/// 测试摘要。
const ISO: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";
/// 产物摘要。
const KERNEL: &str = "cd34cd34cd34cd34cd34cd34cd34cd34cd34cd34cd34cd34cd34cd34cd34cd34";

/// 登记、去重、抽取缓存、级联删除与上传记录。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn iso_lifecycle() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let isos = store.isos();
    let new = NewIso {
        sha256: ISO.to_uppercase(),
        size: 4096,
        label: "PVE".to_owned(),
        volume_id: "PVE".to_owned(),
        hybrid: true,
        detected_family: "pve".to_owned(),
        detection: json!({"family": "pve"}),
        uploaded_by: "admin".to_owned(),
    };
    let id = isos.insert(&new, at(1))?;
    ensure_eq(&isos.insert(&new, at(2))?, &id, "同内容去重")?;
    let listed = isos.list()?;
    ensure_eq(&listed.len(), &1, "一条")?;
    ensure(
        listed
            .first()
            .is_some_and(|iso| iso.sha256 == ISO && iso.hybrid),
        "摘要规范为小写",
    )?;
    let key = ExtractKey {
        iso_sha256: ISO.to_owned(),
        kind: "kernel".to_owned(),
        source_path: "/boot/linux26".to_owned(),
        embed_name: String::new(),
    };
    isos.put_extract(
        &ExtractRecord {
            key: key.clone(),
            sha256: KERNEL.to_owned(),
            size: 10,
        },
        at(3),
    )?;
    ensure(
        isos.extract(&key)?
            .is_some_and(|item| item.sha256 == KERNEL),
        "按键命中",
    )?;
    ensure(isos.extract_by_sha(KERNEL)?.is_some(), "按产物摘要命中")?;
    ensure_eq(
        &isos.delete(ISO, at(4))?,
        &vec![KERNEL.to_owned()],
        "删除返回产物",
    )?;
    ensure(
        isos.list()?.is_empty() && isos.live(ISO)?.is_none(),
        "已删除",
    )?;
    ensure(isos.extract_by_sha(KERNEL)?.is_none(), "抽取记录级联清理")?;
    ensure(isos.delete(ISO, at(5)).is_err(), "重复删除报不存在")?;
    ensure(isos.insert(&new, at(6))? != id, "删除后可重新上传")?;
    Ok(())
}

/// 分块上传记录。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn upload_records() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let isos = store.isos();
    let upload = UploadRecord {
        id: "u1".to_owned(),
        label: "PVE".to_owned(),
        size: 4096,
        expected_sha256: Some(ISO.to_owned()),
        created_by: "admin".to_owned(),
        created_at: at(7),
        updated_at: at(7),
    };
    isos.begin_upload(&upload)?;
    isos.touch_upload("u1", at(8))?;
    ensure_eq(&isos.upload("u1")?.updated_at, &at(8), "刷新时间")?;
    ensure_eq(&isos.uploads()?.len(), &1, "进行中一条")?;
    isos.end_upload("u1")?;
    ensure(isos.upload("u1").is_err(), "结束后不存在")?;
    Ok(())
}
