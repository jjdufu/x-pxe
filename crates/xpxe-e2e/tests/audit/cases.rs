//! 完整装机后停止控制面，以 `audit verify` / `audit export` 的同一路径离线校验审计链。

use xpxe_e2e::Outcome;
use xpxe_e2e::flow::{LanCase, lan_install, run};
use xpxe_e2e::harness::{Harness, Launch, START};
use xpxe_store::{MasterKey, Store};
use xpxe_test_support::{ensure, ensure_eq};

/// 审计链完整且可导出。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn audit_chain_verifies_offline() -> Outcome {
    run(async {
        let mut harness = Harness::start(Launch::default()).await?;
        lan_install(
            &harness,
            LanCase {
                hostname: "e2e-audit",
                mac: "52:54:00:77:01:e2",
                firmware: "uefi",
                pxe_fixture: "existing-ovmf-e1000e-01-discover-67",
                ipxe_fixture: "existing-ovmf-e1000e-05-discover-67-ipxe",
                file_suffix: "/ipxe.efi",
                firmware_fixture: "ipxe/snponly.efi",
            },
        )
        .await?;
        harness.stop().await;
        let (key, _warnings) = MasterKey::load(&harness.file("master.key"))?;
        let store = Store::open(&harness.data_dir(), key)?;
        let report = store.audit().verify()?;
        ensure_eq(&report.first_broken, &None, "审计链完整")?;
        let mut exported = Vec::new();
        let rows = store.audit().export_since(START, &mut exported)?;
        ensure_eq(&rows, &report.checked, "导出条数等于校验条数")?;
        let text = String::from_utf8(exported)?;
        for action in ["auth.login", "node.armed", "asset.activate"] {
            ensure(text.contains(action), &format!("审计包含 {action}"))?;
        }
        Ok(())
    })
}
