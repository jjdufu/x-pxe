//! 配置版本链。

use xpxe_store::StoreError;
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::support::{Picky, by, open, picky};

/// 相同内容不产生新版本；不同内容递增。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn identical_content_reuses_head() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let configs = store.configs();
    let first = configs.create("bmc-a", &picky("a", 1), &by("初版", 0))?;
    ensure_eq(&first.version, &1, "首版本")?;
    let same = configs.save(first.object_id, &picky("a", 1), 1, &by("重复", 1))?;
    ensure_eq(&same, &first, "相同内容返回头版本")?;
    let second = configs.save(first.object_id, &picky("a", 2), 1, &by("改值", 2))?;
    ensure_eq(&second.version, &2, "新版本")?;
    ensure_eq(&configs.versions(first.object_id)?.len(), &2, "历史两条")?;
    Ok(())
}

/// 头版本过期时返回冲突，并给出当前头版本。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn stale_head_conflicts() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let configs = store.configs();
    let first = configs.create("bmc-a", &picky("a", 1), &by("初版", 0))?;
    let _second = configs.save(first.object_id, &picky("a", 2), 1, &by("甲", 1))?;
    let stale = configs.save(first.object_id, &picky("a", 3), 1, &by("乙", 2));
    ensure_eq(
        &stale,
        &Err(StoreError::Conflict { current_head: 2 }),
        "冲突",
    )?;
    Ok(())
}

/// 回滚生成新版本，旧版本内容与历史不变；差异可比较。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn rollback_appends_and_keeps_history() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let configs = store.configs();
    let first = configs.create("bmc-a", &picky("a", 1), &by("初版", 0))?;
    let id = first.object_id;
    let _second = configs.save(id, &picky("a", 2), 1, &by("改", 1))?;
    let before = configs.versions(id)?;
    let rolled = configs.rollback::<Picky>(id, 1, 2, &by("误改", 2))?;
    ensure_eq(&rolled.version, &3, "回滚是新版本")?;
    ensure_eq(
        &rolled.content_sha256,
        &first.content_sha256,
        "内容同版本 1",
    )?;
    let after = configs.versions(id)?;
    ensure_eq(&after.get(1..), &Some(before.as_slice()), "旧历史不变")?;
    let head = after.first().ok_or("无历史")?;
    ensure(head.comment.contains("回滚到版本 1"), &head.comment)?;
    ensure_eq(&head.parent_version, &Some(2), "父版本")?;
    ensure_eq(
        &configs.load::<Picky>(id, 2)?,
        &picky("a", 2),
        "版本 2 内容",
    )?;
    let changes = configs.diff::<Picky>(id, 2, 3)?;
    ensure_eq(&changes.len(), &1, "一处差异")?;
    ensure_eq(
        &changes.first().map(|change| change.path.as_str()),
        &Some("/value"),
        "差异路径",
    )?;
    let (head_ref, doc) = configs.head::<Picky>(id)?;
    ensure_eq(&head_ref.version, &3, "头版本")?;
    ensure_eq(&doc, &picky("a", 1), "头内容")?;
    Ok(())
}

/// 校验失败的文档不写入任何东西（包括对象行）。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn invalid_document_writes_nothing() -> TestResult {
    let dir = TempDataDir::new()?;
    let store = open(dir.path())?;
    let configs = store.configs();
    ensure(
        matches!(
            configs.create("bad", &picky("", 1), &by("非法", 0)),
            Err(StoreError::Config(_))
        ),
        "校验失败",
    )?;
    ensure(
        configs.list(xpxe_domain::ConfigKind::Bmc)?.is_empty(),
        "对象行已回滚",
    )?;
    let _recreated = configs.create("bad", &picky("ok", 1), &by("合法", 1))?;
    ensure_eq(
        &configs.list(xpxe_domain::ConfigKind::Bmc)?.len(),
        &1,
        "同名可再建",
    )?;
    Ok(())
}
