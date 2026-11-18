//! 下载流程（测试内 HTTPS 服务 + 自定义根）：重定向白名单、超限中止、断流、ETag、内容寻址落盘、
//! 以及经 HTTPS 镜像的完整对齐检查。

use std::sync::Arc;
use std::time::Duration;

use xpxe_assets::{
    FetchPolicy, Fetcher, FileTarget, Ingest, Mirror, Origin, PACKAGES_XZ, TrustStore,
};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use crate::server::{Route, ok, redirect, response, start};
use crate::support::{NOW, code, fixture, runtime};

/// 正文。
const BODY: &[u8] = b"hello x-pxe";

/// 测试策略：白名单只含测试主机，超时缩短。
fn policy() -> FetchPolicy {
    FetchPolicy {
        offline: false,
        official_hosts: vec!["localhost".to_owned(), "127.0.0.1".to_owned()],
        header_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(5),
    }
}

/// 测试下载器（只信任测试 CA）。
///
/// # Errors
///
/// CA 加载失败时返回错误。
fn fetcher() -> TestResult<Fetcher> {
    Ok(Fetcher::with_root_pem(policy(), &fixture("tls/ca.pem")?)?)
}

/// 测试路由。
fn routes() -> Route {
    Arc::new(|path: &str, port: u16| match path {
        "/ok" => ok(
            &[
                ("ETag", "\"v1\""),
                ("Last-Modified", "Wed, 18 Nov 2026 00:00:00 GMT"),
            ],
            BODY,
        ),
        "/hop" => redirect(&format!("https://127.0.0.1:{port}/ok")),
        "/relative" => redirect("/ok"),
        "/evil" => redirect(&format!("https://evil.test:{port}/ok")),
        "/plain" => redirect(&format!("http://localhost:{port}/ok")),
        "/loop" => redirect("/loop"),
        "/big" => response("200 OK", &[], b"", Some(64 * 1024 * 1024)),
        "/stream" => response("200 OK", &[], &vec![b'x'; 2 * 1024 * 1024], None),
        "/broken" => response("200 OK", &[], b"short", Some(1000)),
        _ => response("404 Not Found", &[], b"<html>404</html>", Some(16)),
    })
}

/// 内存下载、重定向（绝对与相对）、ETag 记录与策略拒绝。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn redirects_and_policy() -> TestResult {
    let rt = runtime()?;
    rt.block_on(async {
        let port = start(routes()).await?;
        let client = fetcher()?;
        let base = format!("https://localhost:{port}");
        let got = client
            .get_bytes(&format!("{base}/ok"), Origin::Official, 1024)
            .await?;
        ensure_eq(got.data.as_slice(), BODY, "body")?;
        ensure_eq(&got.meta.etag.as_deref(), &Some("\"v1\""), "etag")?;
        ensure(got.meta.last_modified.is_some(), "last-modified")?;
        ensure_eq(
            got.meta.sha256.as_str(),
            hex::encode(<sha2::Sha256 as sha2::Digest>::digest(BODY)).as_str(),
            "sha",
        )?;
        let hop = client
            .get_bytes(&format!("{base}/hop"), Origin::Official, 1024)
            .await?;
        ensure_eq(
            hop.meta.final_url.as_str(),
            format!("https://127.0.0.1:{port}/ok").as_str(),
            "final url",
        )?;
        let relative = client
            .get_bytes(&format!("{base}/relative"), Origin::Official, 1024)
            .await?;
        ensure_eq(relative.data.as_slice(), BODY, "relative")?;
        for (path, want) in [
            ("/evil", "host_denied"),
            ("/plain", "host_denied"),
            ("/loop", "too_many_redirects"),
            ("/missing", "http_status"),
        ] {
            let result = client
                .get_bytes(&format!("{base}{path}"), Origin::Official, 1024)
                .await;
            ensure_eq(&code(&result), &Some(want), path)?;
        }
        let offline = Fetcher::with_root_pem(
            FetchPolicy {
                offline: true,
                ..policy()
            },
            &fixture("tls/ca.pem")?,
        )?;
        let result = offline
            .get_bytes(&format!("{base}/ok"), Origin::Official, 1024)
            .await;
        ensure_eq(&code(&result), &Some("host_denied"), "offline")?;
        Ok(())
    })
}

/// 超大文件（声明或流式）立即中止，断流报错，临时文件被清理；成功时按哈希落盘。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn limits_and_files() -> TestResult {
    let rt = runtime()?;
    rt.block_on(async {
        let port = start(routes()).await?;
        let client = fetcher()?;
        let base = format!("https://localhost:{port}");
        let dir = TempDataDir::new()?;
        let target = |limit: u64, expected: Option<&'static str>| FileTarget {
            dir: dir.path(),
            limit,
            expected_sha256: expected,
        };
        for (path, want) in [
            ("/big", "too_large"),
            ("/stream", "too_large"),
            ("/broken", "stream_broken"),
        ] {
            let result = client
                .get_file(
                    &format!("{base}{path}"),
                    Origin::Official,
                    target(1024 * 1024, None),
                )
                .await;
            ensure_eq(&code(&result), &Some(want), path)?;
        }
        let stored = client
            .get_file(&format!("{base}/ok"), Origin::Official, target(1024, None))
            .await?;
        ensure_eq(&std::fs::read(&stored.path)?, &BODY.to_vec(), "stored")?;
        ensure(
            stored.path.ends_with(&stored.meta.sha256),
            "content addressed",
        )?;
        let wrong = client
            .get_file(
                &format!("{base}/ok"),
                Origin::Official,
                target(
                    1024,
                    Some("0000000000000000000000000000000000000000000000000000000000000000"),
                ),
            )
            .await;
        ensure_eq(&code(&wrong), &Some("hash_mismatch"), "mismatch")?;
        let leftovers = std::fs::read_dir(dir.path().join(".tmp"))?.count();
        ensure_eq(&leftovers, &0, "tmp cleaned")?;
        Ok(())
    })
}

/// 经 HTTPS 镜像（真实 `InRelease` + `Packages.xz`）完成对齐检查。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn mirror_alignment_end_to_end() -> TestResult {
    let inrelease = Arc::new(fixture("debian/InRelease")?);
    let packages = Arc::new(fixture("debian/Packages.xz")?);
    let route: Route = Arc::new(move |path: &str, _port: u16| {
        if path == "/debian/dists/trixie/InRelease" {
            ok(&[], &inrelease)
        } else if path == format!("/debian/dists/trixie/{PACKAGES_XZ}") {
            ok(&[], &packages)
        } else {
            response("404 Not Found", &[], b"", Some(0))
        }
    });
    let rt = runtime()?;
    rt.block_on(async {
        let port = start(route).await?;
        let client = fetcher()?;
        let trust = TrustStore::debian()?;
        let dir = TempDataDir::new()?;
        let ingest = Ingest {
            fetcher: &client,
            trust: &trust,
            dir: dir.path(),
        };
        let url = format!("https://localhost:{port}/debian");
        let mirror = Mirror {
            url: &url,
            suite: "trixie",
            signed: true,
        };
        let report = ingest
            .alignment(mirror, "6.12.107+deb13-amd64", NOW)
            .await?;
        ensure(report.alignment.is_aligned(), "aligned")?;
        ensure(report.signer.is_some(), "signed")?;
        let old = ingest.alignment(mirror, "6.12.38+deb13-amd64", NOW).await?;
        ensure(!old.alignment.is_aligned(), "old misaligned")?;
        Ok(())
    })
}
