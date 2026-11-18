//! DMTF Redfish Mockup（`public-rackmount1`）资源发现回归。

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use xpxe_redfish::{ProfileId, Session};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use crate::server::{Handler, Incoming, config, runtime, start};

/// 按路径返回 Mockup 文件（`/redfish/v1/X` → `X/index.json`）；不支持创建会话。
fn mockup_handler() -> Handler {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mockup/public-rackmount1");
    Arc::new(move |request: &Incoming| {
        if request.method != "GET" {
            return (405, Vec::new(), Value::Null);
        }
        let rest = request
            .path
            .trim_end_matches('/')
            .strip_prefix("/redfish/v1")
            .unwrap_or("/missing")
            .trim_start_matches('/');
        let file = dir.join(rest).join("index.json");
        std::fs::read(file)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .map_or_else(
                || (404, Vec::new(), Value::Null),
                |json| (200, Vec::new(), json),
            )
    })
}

/// 发现唯一系统 `437XR1138R2`、System 下的 CD1（跳过 Floppy1），模式 UEFI，允许 Cd。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn public_rackmount1_discovery() -> TestResult {
    runtime()?.block_on(async {
        let port = start("bmc", mockup_handler()).await?;
        let session = Session::open(&config(port, "bmc")?).await?;
        ensure(!session.uses_token(), "Mockup 不支持创建会话，回落 Basic")?;
        ensure_eq(
            &session.profile_id(),
            &ProfileId::Generic,
            "Vendor 为空 → Generic",
        )?;
        let target = session.discover().await?;
        ensure_eq(
            &target.system.as_str(),
            &"/redfish/v1/Systems/437XR1138R2",
            "系统",
        )?;
        ensure_eq(
            &target.vmedia.as_str(),
            &"/redfish/v1/Systems/437XR1138R2/VirtualMedia/CD1",
            "CD 介质",
        )?;
        ensure_eq(&target.boot_mode.as_deref(), &Some("UEFI"), "模式")?;
        ensure_eq(&target.power_state.as_deref(), &Some("On"), "电源")?;
        Ok(())
    })
}
