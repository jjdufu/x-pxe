//! 未配置证书时为 HTTP；配置测试证书后同一端口为 HTTPS，Cookie 带 `Secure`、响应带 HSTS；证书配置不完整或不可用时启动失败。

use std::sync::Arc;

use xpxe_control::app::{App, Options};
use xpxe_control::server;
use xpxe_e2e::Outcome;
use xpxe_e2e::flow::run;
use xpxe_e2e::harness::{Harness, Launch, settings_for, tls_fixture};
use xpxe_test_support::{TempDataDir, ensure};

/// 首次登录的 `Set-Cookie` 与 HSTS 头。
///
/// # Errors
///
/// 请求失败时返回错误。
async fn login_headers(harness: &Harness) -> Outcome<(String, Option<String>)> {
    let body =
        serde_json::json!({ "username": "admin", "password": xpxe_e2e::harness::INITIAL_PASSWORD });
    let reply = harness
        .admin("POST", "/api/v1/auth/login", None, Some(&body))
        .await?;
    ensure(
        reply.status == 200,
        &format!("登录 {}：{}", reply.status, reply.text()),
    )?;
    let cookie = reply.header("set-cookie").unwrap_or_default().to_owned();
    Ok((
        cookie,
        reply.header("strict-transport-security").map(str::to_owned),
    ))
}

/// HTTP：Cookie 含 `HttpOnly`、`SameSite=Strict`，不含 `Secure`，无 HSTS。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn plain_http_without_certificate() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        let (cookie, hsts) = login_headers(&harness).await?;
        ensure(
            cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"),
            &format!("Cookie 属性：{cookie}"),
        )?;
        ensure(!cookie.contains("Secure"), "HTTP 不带 Secure")?;
        ensure(hsts.is_none(), "HTTP 无 HSTS")?;
        Ok(())
    })
}

/// HTTPS：同一端口 TLS，Cookie 含 `Secure`，响应带 HSTS。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn https_with_certificate() -> Outcome {
    run(async {
        let (cert, key, _ca) = tls_fixture();
        let harness = Harness::start(Launch {
            tls: Some((cert, key)),
            cert_only: false,
        })
        .await?;
        let (cookie, hsts) = login_headers(&harness).await?;
        ensure(
            cookie.contains("Secure") && cookie.contains("HttpOnly"),
            &format!("Cookie 属性：{cookie}"),
        )?;
        ensure(
            hsts.is_some_and(|value| value.contains("max-age=")),
            "HTTPS 带 HSTS",
        )?;
        Ok(())
    })
}

/// 只配置证书时配置解析失败；私钥不可用时启动失败。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn incomplete_or_bad_certificate_fails() -> Outcome {
    run(async {
        let (cert, key, ca) = tls_fixture();
        let dir = TempDataDir::new()?;
        let only_cert = settings_for(
            dir.path(),
            &Launch {
                tls: Some((cert.clone(), key)),
                cert_only: true,
            },
        );
        ensure(only_cert.is_err(), "只配置证书应失败")?;
        let bad_dir = TempDataDir::new()?;
        let settings = settings_for(
            bad_dir.path(),
            &Launch {
                tls: Some((cert, ca)),
                cert_only: false,
            },
        )?;
        let app: Arc<App> =
            App::open(settings, Options::default()).map_err(|error| error.to_string())?;
        ensure(server::start(app).await.is_err(), "私钥不可用时启动失败")?;
        Ok(())
    })
}
