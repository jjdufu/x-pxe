//! 命令行解析测试。

use std::path::PathBuf;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{Command, parse, parse_since};

/// 把字面量转为参数列表。
fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// 子命令解析。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn parses_subcommands() -> TestResult {
    ensure_eq(&parse(&args(&["serve"])), &Ok(Command::Serve), "serve")?;
    ensure_eq(
        &parse(&args(&["audit", "verify"])),
        &Ok(Command::AuditVerify),
        "audit verify",
    )?;
    ensure_eq(
        &parse(&args(&[
            "rotate-master-key",
            "--old-file",
            "/a",
            "--new-file",
            "/b",
        ])),
        &Ok(Command::Rotate {
            old: PathBuf::from("/a"),
            new: PathBuf::from("/b"),
        }),
        "rotate",
    )?;
    ensure_eq(
        &parse(&args(&["backup", "--out", "/x.db"])),
        &Ok(Command::Backup {
            out: PathBuf::from("/x.db"),
        }),
        "backup",
    )?;
    ensure(parse(&args(&["backup"])).is_err(), "backup 缺 --out")?;
    ensure(parse(&args(&["bogus"])).is_err(), "未知子命令")?;
    Ok(())
}

/// `--since` 两种格式。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn since_accepts_rfc3339_and_unix() -> TestResult {
    ensure_eq(
        &parse_since("1700000000").map(xpxe_domain::Timestamp::unix_seconds),
        &Ok(1_700_000_000),
        "unix 秒",
    )?;
    ensure_eq(
        &parse_since("2023-11-14T22:13:20Z").map(xpxe_domain::Timestamp::unix_seconds),
        &Ok(1_700_000_000),
        "RFC 3339",
    )?;
    ensure(parse_since("昨天").is_err(), "非法格式")?;
    Ok(())
}
