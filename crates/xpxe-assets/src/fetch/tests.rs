//! 下载策略单元测试（真实 HTTPS 流程见集成测试）。

use http::{HeaderMap, HeaderValue, Uri, header};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{FetchPolicy, Origin, redirect_target};

/// 白名单、协议与离线模式。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn policy_rules() -> TestResult {
    let online = FetchPolicy::standard(false);
    let offline = FetchPolicy::standard(true);
    let ipxe: Uri = "https://boot.ipxe.org/undionly.kpxe".parse()?;
    let plain: Uri = "http://boot.ipxe.org/undionly.kpxe".parse()?;
    let other: Uri = "https://example.com/x".parse()?;
    let nexus: Uri = "http://192.168.2.5:8081/repository/debian".parse()?;
    ensure(online.check(&ipxe, Origin::Official).is_ok(), "official ok")?;
    ensure(
        online.check(&plain, Origin::Official).is_err(),
        "official http",
    )?;
    ensure(
        online.check(&other, Origin::Official).is_err(),
        "not whitelisted",
    )?;
    ensure(online.check(&other, Origin::Url).is_ok(), "custom https")?;
    ensure(online.check(&nexus, Origin::Url).is_err(), "custom http")?;
    ensure(
        offline.check(&ipxe, Origin::Official).is_err(),
        "offline official",
    )?;
    ensure(
        offline.check(&ipxe, Origin::Url).is_err(),
        "offline official via url",
    )?;
    ensure(
        offline.check(&other, Origin::Url).is_ok(),
        "offline intranet",
    )?;
    ensure(
        offline.check(&nexus, Origin::Mirror).is_ok(),
        "offline mirror",
    )?;
    Ok(())
}

/// 绝对与路径式 `Location`，拒绝协议相对与相对路径。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn redirect_locations() -> TestResult {
    let current: Uri = "https://github.com/ipxe/ipxe/releases/download/v2/ipxe.efi".parse()?;
    let mut headers = HeaderMap::new();
    let _old = headers.insert(header::LOCATION, HeaderValue::from_static("/other"));
    ensure_eq(
        redirect_target(&current, &headers)?.to_string().as_str(),
        "https://github.com/other",
        "path",
    )?;
    let _second = headers.insert(header::LOCATION, HeaderValue::from_static("//evil.test/x"));
    ensure(
        redirect_target(&current, &headers).is_err(),
        "protocol relative",
    )?;
    let _third = headers.insert(header::LOCATION, HeaderValue::from_static("x"));
    ensure(redirect_target(&current, &headers).is_err(), "relative")?;
    Ok(())
}
