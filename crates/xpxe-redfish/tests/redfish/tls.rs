//! 证书固定：探测不发凭据、正确指纹通过、错误指纹/更换证书失败并报告证书变化、凭据错误。

use std::time::Duration;

use xpxe_redfish::{Endpoint, ProfileId, Session, probe};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use crate::bmc::{Quirk, find, mock};
use crate::server::{config, credentials, fingerprint, runtime, start};

/// `probe` 返回证书指纹、主题与服务信息；请求只有匿名的 `GET /redfish/v1`。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn probe_without_credentials() -> TestResult {
    runtime()?.block_on(async {
        let (state, handler) = mock("Dell Inc.", &[]);
        let port = start("bmc", handler).await?;
        let endpoint = Endpoint::parse(&format!("https://127.0.0.1:{port}/"))?;
        let report = probe(&endpoint, Duration::from_secs(5)).await?;
        ensure_eq(&report.fingerprint, &fingerprint("bmc")?, "指纹")?;
        ensure(report.certificate.subject.contains("CN=127.0.0.1"), "主题")?;
        ensure_eq(&report.vendor.as_deref(), &Some("Dell Inc."), "Vendor")?;
        ensure_eq(
            &report.suggested_profile,
            &ProfileId::DellIdrac9,
            "建议画像",
        )?;
        let log = state.lock().map_err(|_poisoned| "锁中毒")?.log.clone();
        ensure_eq(&log.len(), &1, "只有一个请求")?;
        ensure(
            log.iter().all(|item| {
                item.method == "GET"
                    && item.path == "/redfish/v1"
                    && item.authorization.is_none()
                    && item.token.is_none()
                    && item.body.is_null()
            }),
            "探测不含 Authorization 头与凭据",
        )?;
        Ok(())
    })
}

/// 正确指纹：建立令牌会话，结束时删除会话。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn correct_pin_token_session() -> TestResult {
    runtime()?.block_on(async {
        let (state, handler) = mock("", &[]);
        let port = start("bmc", handler).await?;
        let session = Session::open(&config(port, "bmc")?).await?;
        ensure(session.uses_token(), "令牌会话")?;
        let _target = session.discover().await?;
        session.close().await?;
        ensure_eq(&find(&state, "DELETE", "/Sessions/1").len(), &1, "删除会话")?;
        ensure(
            find(&state, "GET", "/Systems")
                .iter()
                .all(|item| item.token.as_deref() == Some("tok-1") && item.authorization.is_none()),
            "后续请求只带令牌",
        )?;
        Ok(())
    })
}

/// 错误指纹或服务端更换证书：`cert_changed`，且不发送任何 HTTP 请求（凭据不外泄）。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn wrong_pin_reports_cert_change() -> TestResult {
    runtime()?.block_on(async {
        let (state, handler) = mock("", &[]);
        let port = start("other", handler).await?;
        let error = Session::open(&config(port, "bmc")?)
            .await
            .err()
            .ok_or("错误指纹不应连上")?;
        ensure_eq(&error.code(), &"cert_changed", "错误码")?;
        ensure(
            error.message().contains(&fingerprint("other")?.to_string()),
            "报告实际指纹",
        )?;
        ensure(
            state.lock().map_err(|_poisoned| "锁中毒")?.log.is_empty(),
            "握手失败前不发请求",
        )?;
        Ok(())
    })
}

/// 凭据错误：创建会话返回 401 → `unauthorized`。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn bad_credentials() -> TestResult {
    runtime()?.block_on(async {
        let (_state, handler) = mock("", &[]);
        let port = start("bmc", handler).await?;
        let mut bad = config(port, "bmc")?;
        bad.credentials = xpxe_redfish::Credentials::new("admin".to_owned(), "wrong".to_owned());
        let error = Session::open(&bad).await.err().ok_or("错误密码不应登录")?;
        ensure_eq(&error.code(), &"unauthorized", "错误码")?;
        ensure(
            !format!("{:?}", credentials()).contains("secret"),
            "Debug 不泄露密码",
        )?;
        Ok(())
    })
}

/// 没有 `SessionService` 时回落 Basic。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn basic_fallback() -> TestResult {
    runtime()?.block_on(async {
        let (state, handler) = mock("", &[Quirk::NoSessions]);
        let port = start("bmc", handler).await?;
        let session = Session::open(&config(port, "bmc")?).await?;
        ensure(!session.uses_token(), "Basic")?;
        let _target = session.discover().await?;
        ensure(
            find(&state, "GET", "/Systems")
                .iter()
                .all(|item| item.authorization.as_deref() == Some("Basic YWRtaW46c2VjcmV0")),
            "请求带 Basic",
        )?;
        session.close().await?;
        ensure(find(&state, "DELETE", "").is_empty(), "Basic 模式不删会话")?;
        Ok(())
    })
}
