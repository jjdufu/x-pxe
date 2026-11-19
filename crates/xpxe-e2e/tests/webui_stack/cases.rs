//! 联调栈：固定端口启动、托管前端目录、初始口令可登录、netboot 夹具已暂存、心跳健康。

use std::net::{SocketAddr, TcpListener};

use serde_json::json;
use xpxe_e2e::flow::run;
use xpxe_e2e::harness::INITIAL_PASSWORD;
use xpxe_e2e::http::{Call, send};
use xpxe_e2e::stack::{Config, Stack, config_from};
use xpxe_e2e::{Outcome, stack};
use xpxe_test_support::{TempDataDir, ensure, ensure_eq};

/// 空闲本地地址。
///
/// # Errors
///
/// 绑定失败时返回错误。
fn free_addr() -> Outcome<SocketAddr> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?)
}

/// 参数解析：缺省值与覆盖。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn config_defaults() -> Outcome {
    let root = std::path::Path::new("/repo");
    let config = config_from(&[], root)?;
    ensure_eq(
        &config.admin.to_string(),
        &"127.0.0.1:8088".to_owned(),
        "管理端缺省",
    )?;
    ensure_eq(
        &config.boot.to_string(),
        &"127.0.0.1:8080".to_owned(),
        "引导端缺省",
    )?;
    ensure_eq(&config.web, &root.join("web/dist"), "前端目录缺省")?;
    let custom = config_from(&["--boot".to_owned(), "127.0.0.1:9".to_owned()], root)?;
    ensure_eq(&custom.boot.port(), &9, "覆盖引导端")?;
    Ok(())
}

/// 启动冒烟。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn stack_starts() -> Outcome {
    run(async {
        let dir = TempDataDir::new()?;
        let web = dir.path().join("web");
        std::fs::create_dir_all(&web)?;
        std::fs::write(
            web.join("index.html"),
            "<!doctype html><title>x-pxe</title>",
        )?;
        let config = Config {
            dir: dir.path().join("stack"),
            admin: free_addr()?,
            boot: free_addr()?,
            web,
        };
        let running = Stack::start(&config).await?;
        let info = running.info();
        ensure_eq(
            &info.get("allowed_iface"),
            &Some(&json!(stack::ALLOWED_IFACE)),
            "白名单网卡",
        )?;
        ensure(
            !info.to_string().contains(INITIAL_PASSWORD),
            "连接信息不含口令",
        )?;
        let page = send(
            config.admin,
            None,
            &Call {
                method: "GET",
                path: "/",
                headers: Vec::new(),
                body: Vec::new(),
            },
        )
        .await?;
        ensure(page.text().contains("x-pxe"), "托管前端目录")?;
        let login = send(
            config.admin,
            None,
            &Call {
                method: "POST",
                path: "/api/v1/auth/login",
                headers: vec![("Content-Type", "application/json".to_owned())],
                body: json!({ "username": "admin", "password": INITIAL_PASSWORD })
                    .to_string()
                    .into_bytes(),
            },
        )
        .await?;
        ensure_eq(&login.status, &200, "初始口令登录")?;
        let assets = running.app().store().assets().list()?;
        ensure_eq(&assets.len(), &1, "netboot 夹具已暂存")?;
        running.shutdown().await;
        Ok(())
    })
}
