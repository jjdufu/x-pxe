//! `simulate-install` 解析助手测试。

use serde_json::json;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{enabled_modules, parse_base, parse_response, session_of, token_before};

/// 引导端地址解析。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn parses_base() -> TestResult {
    let plain = parse_base("http://127.0.0.1:8080")?;
    ensure_eq(&plain.host_port, &"127.0.0.1:8080".to_owned(), "主机")?;
    ensure_eq(&plain.prefix, &String::new(), "无前缀")?;
    let prefixed = parse_base("http://boot.lab:80/x/")?;
    ensure_eq(&prefixed.prefix, &"/x".to_owned(), "前缀")?;
    ensure(parse_base("https://a").is_err(), "拒绝 https")?;
    ensure(parse_base("http:///p").is_err(), "缺少主机")?;
    Ok(())
}

/// 令牌、会话与模块提取。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn extracts_tokens() -> TestResult {
    let script = "#!ipxe\nchain http://127.0.0.1:8080/p/AbC123/boot.ipxe\n";
    ensure_eq(
        &token_before(script, "boot.ipxe"),
        &Some("AbC123".to_owned()),
        "boot 令牌",
    )?;
    ensure_eq(
        &token_before("#!ipxe\nexit\n", "boot.ipxe"),
        &None,
        "未 arm",
    )?;
    let late = "wget -O /target/var/lib/xpxe/plan.json 'http://h/p/Agent9/plan.json'\n";
    ensure_eq(
        &token_before(late, "plan.json"),
        &Some("Agent9".to_owned()),
        "agent 令牌",
    )?;
    let preseed = "d-i preseed/early_command string wget --post-data 'event=installer.started event_id=0190abcd.installer' x";
    ensure_eq(&session_of(preseed), &Some("0190abcd".to_owned()), "会话")?;
    let plan = json!({ "modules": [
        { "id": "apt_sources", "enabled": true },
        { "id": "docs", "enabled": false },
        { "id": "sudoers" },
    ] });
    ensure_eq(
        &enabled_modules(&plan),
        &vec!["apt_sources".to_owned(), "sudoers".to_owned()],
        "启用模块",
    )?;
    Ok(())
}

/// 响应解析（含 chunked）。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn parses_responses() -> TestResult {
    let plain = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")?;
    ensure_eq(&plain.status, &200, "状态")?;
    ensure_eq(&plain.body, &"ok".to_owned(), "正文")?;
    let chunked = parse_response(
        b"HTTP/1.1 410 Gone\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
    )?;
    ensure_eq(&chunked.status, &410, "状态 410")?;
    ensure_eq(&chunked.body, &"abcde".to_owned(), "chunked 正文")?;
    ensure(parse_response(b"garbage").is_err(), "非法响应")?;
    Ok(())
}
