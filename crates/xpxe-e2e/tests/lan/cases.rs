//! LAN 全链路用例。

use xpxe_e2e::bootnet::Sim;
use xpxe_e2e::flow::{LanCase, audit_ok, lan_install, register_and_arm, run};
use xpxe_e2e::harness::{Harness, Launch};
use xpxe_e2e::{Outcome, seed};
use xpxe_test_support::{ensure, ensure_eq};

/// OVMF：`fw/<sha12>/ipxe.efi` → iPXE → 安装 → `Ready`。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn lan_uefi() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        lan_install(
            &harness,
            LanCase {
                hostname: "e2e-uefi",
                mac: "52:54:00:77:01:e2",
                firmware: "uefi",
                pxe_fixture: "existing-ovmf-e1000e-01-discover-67",
                ipxe_fixture: "existing-ovmf-e1000e-05-discover-67-ipxe",
                file_suffix: "/ipxe.efi",
                firmware_fixture: "ipxe/snponly.efi",
            },
        )
        .await
    })
}

/// `SeaBIOS`：`fw/<sha12>/undionly.kpxe` → iPXE → 安装 → `Ready`。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn lan_bios() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        lan_install(
            &harness,
            LanCase {
                hostname: "e2e-bios",
                mac: "52:54:00:77:01:b6",
                firmware: "bios",
                pxe_fixture: "constructed-bios-discover-67",
                ipxe_fixture: "existing-seabios-virtio-01-discover-67-ipxe",
                file_suffix: "/undionly.kpxe",
                firmware_fixture: "ipxe/undionly.kpxe",
            },
        )
        .await
    })
}

/// UEFI HTTP Boot：应答为含端口的引导端 URL，Opt60 为 `HTTPClient`，URL 可直接下载固件。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn http_boot() -> Outcome {
    run(async {
        let harness = Harness::start(Launch::default()).await?;
        let (login, ids) = seed::lan(&harness).await?;
        let spec = seed::lan_spec("e2e-http", "52:54:00:77:01:d2", "uefi");
        let (_node, _session) = register_and_arm(&harness, &login, ids, &spec).await?;
        let mut sim = Sim::new(harness.running()?.internal_addr(), &harness.token_file())?;
        sim.sync().await?;
        let answer = sim
            .replay("ovmf-http-e1000e-01-discover-67", harness.app()?.now())?
            .ok_or("HTTP Boot 无应答")?;
        ensure_eq(&answer.vendor_class.as_str(), &"HTTPClient", "Opt60")?;
        let port = harness.running()?.boot_addr().port();
        let prefix = format!("http://127.0.0.1:{port}/fw/");
        ensure(
            answer.file.starts_with(&prefix) && answer.file.ends_with("ipxe.efi"),
            &format!("URL {}", answer.file),
        )?;
        let path = answer
            .file
            .get(prefix.len().saturating_sub(4)..)
            .unwrap_or_default();
        let firmware = harness.boot("GET", path, b"").await?;
        ensure_eq(&firmware.status, &200, "固件 URL 可下载")?;
        ensure(
            firmware.body == seed::asset_fixture("ipxe/snponly.efi")?,
            "字节一致",
        )?;
        audit_ok(&harness, &login).await
    })
}
