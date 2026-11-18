//! `iso`：用 `xpxe-image` 自写的 FAT 与 ISO9660/El Torito 写出器生成 UEFI 引导 ISO。
//!
//! 两种用法：
//!
//! - `--session-fixture <json> --out <boot.iso>`（Step 9）：按会话夹具渲染 `autoexec.ipxe`
//!   （`xpxe-render::redfish_boot`），再用 `xpxe-image::session` 生成正式的会话引导 ISO。
//! - `--efi <ipxe.efi> --script <autoexec.ipxe> --out <boot.iso> [--script-dir root|efi|both]`（Step 2
//!   探针）：脚本放在 ESP 根目录（`root`）、与 iPXE 同目录（`efi`）或两处都放（`both`，默认），用于验证 H5。

use serde::Deserialize;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use xpxe_domain::{BootToken, HttpUrl, MacAddr, NodeNetwork};
use xpxe_image::Timestamp;
use xpxe_image::fat::{self, FileSpec};
use xpxe_image::iso::{self, Entry};
use xpxe_image::session::{BootIsoInput, build_boot_iso, volume_label};
use xpxe_render::redfish_boot::{AutoexecInput, render_autoexec};

use crate::common::{Args, LabError, LabResult, emit};

/// 固定时间戳，保证同输入同输出。
const STAMP: Timestamp = Timestamp {
    year: 2026,
    month: 1,
    day: 1,
    hour: 0,
    minute: 0,
    second: 0,
};

/// 子命令入口：`--efi <ipxe.efi> --script <autoexec.ipxe> --out <boot.iso> [--script-dir root|efi|both]`。
///
/// # Errors
///
/// 读写或镜像生成失败时返回 [`LabError`]。
pub(crate) fn run(args: &Args) -> LabResult {
    if let Some(fixture) = args.value("--session-fixture") {
        return run_session(&fixture, &args.required("--out")?);
    }
    let efi = std::fs::read(args.required("--efi")?)?;
    let script = std::fs::read(args.required("--script")?)?;
    let out = args.required("--out")?;
    let placement = args
        .value("--script-dir")
        .unwrap_or_else(|| "both".to_owned());
    let mut files = vec![FileSpec {
        path: "EFI/BOOT/BOOTX64.EFI",
        data: &efi,
    }];
    match placement.as_str() {
        "root" => files.push(FileSpec {
            path: "autoexec.ipxe",
            data: &script,
        }),
        "efi" => files.push(FileSpec {
            path: "EFI/BOOT/autoexec.ipxe",
            data: &script,
        }),
        "both" => {
            files.push(FileSpec {
                path: "autoexec.ipxe",
                data: &script,
            });
            files.push(FileSpec {
                path: "EFI/BOOT/autoexec.ipxe",
                data: &script,
            });
        }
        other => return Err(LabError(format!("--script-dir 取值非法：{other}"))),
    }
    let esp = fat::build(&files, "XPXE-ESP", STAMP)?;
    let image = iso::build(
        &[
            Entry {
                name: "ESP.IMG",
                data: &esp.bytes,
            },
            Entry {
                name: "AUTOEXEC.IPX",
                data: &script,
            },
        ],
        "ESP.IMG",
        "XPXE_BOOT",
        STAMP,
    )?;
    std::fs::write(&out, &image)?;
    emit(
        &json!({"event": "iso", "out": out, "bytes": image.len(), "esp_bytes": esp.bytes.len(), "fat": format!("{:?}", esp.variant), "script_dir": placement}),
    )
}

/// 会话夹具（模拟控制面在一次 Redfish 部署中掌握的会话数据）。
#[derive(Debug, Deserialize)]
struct SessionFixture {
    /// 会话 ID。
    session_id: String,
    /// 会话创建时间（RFC 3339）。
    created_at: String,
    /// 引导网卡 MAC。
    boot_mac: String,
    /// 节点网络配置。
    network: NodeNetwork,
    /// HTTP 根。
    boot_base: String,
    /// 会话令牌。
    boot_token: String,
    /// `ipxe.efi` 路径（相对路径以夹具所在目录为基准）。
    ipxe_efi: String,
}

/// 由解析错误构造实验室错误。
fn invalid(what: &str, error: &dyn std::fmt::Display) -> LabError {
    LabError(format!("会话夹具 {what} 不合法：{error}"))
}

/// 会话时间 → 镜像时间戳（UTC）。
///
/// # Errors
///
/// 不是 RFC 3339 或超出镜像时间范围时返回错误。
fn stamp_of(text: &str) -> LabResult<Timestamp> {
    let utc = OffsetDateTime::parse(text, &Rfc3339)
        .map_err(|error| invalid("created_at", &error))?
        .to_offset(time::UtcOffset::UTC);
    let year = u16::try_from(utc.year()).map_err(|error| invalid("created_at", &error))?;
    Ok(Timestamp {
        year,
        month: u8::from(utc.month()),
        day: utc.day(),
        hour: utc.hour(),
        minute: utc.minute(),
        second: utc.second(),
    }
    .validate()?)
}

/// `--session-fixture` 模式。
///
/// # Errors
///
/// 夹具不合法、读写失败或镜像生成失败时返回错误。
fn run_session(fixture_path: &str, out: &str) -> LabResult {
    let raw = std::fs::read(fixture_path)?;
    let fixture: SessionFixture =
        serde_json::from_slice(&raw).map_err(|error| invalid("JSON", &error))?;
    let mac = MacAddr::parse(&fixture.boot_mac).map_err(|error| invalid("boot_mac", &error))?;
    let base = HttpUrl::parse(&fixture.boot_base).map_err(|error| invalid("boot_base", &error))?;
    let token =
        BootToken::parse(&fixture.boot_token).map_err(|error| invalid("boot_token", &error))?;
    let script = render_autoexec(&AutoexecInput {
        boot_mac: mac,
        network: &fixture.network,
        boot_base: &base,
        boot_token: &token,
    })
    .map_err(|error| invalid("渲染", &error))?;
    let efi_path = std::path::Path::new(fixture_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(&fixture.ipxe_efi);
    let efi = std::fs::read(&efi_path)?;
    let image = build_boot_iso(&BootIsoInput {
        session_id: &fixture.session_id,
        created: stamp_of(&fixture.created_at)?,
        ipxe_efi: &efi,
        autoexec: script.as_bytes(),
    })?;
    std::fs::write(out, &image)?;
    emit(&json!({
        "event": "session_iso",
        "out": out,
        "bytes": image.len(),
        "sha256": hex::encode(Sha256::digest(&image)),
        "volume_id": volume_label(&fixture.session_id)?,
        "boot_url": format!("{}/p/{}/boot.ipxe", base.as_str().trim_end_matches('/'), token.as_str()),
        "static": matches!(fixture.network, NodeNetwork::Static { .. }),
    }))
}
