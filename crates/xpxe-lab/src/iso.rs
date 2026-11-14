//! `iso`：用 `xpxe-image` 自写的 FAT 与 ISO9660/El Torito 写出器生成 UEFI 引导 ISO。
//!
//! ESP 内容：`EFI/BOOT/BOOTX64.EFI`（iPXE）与 `autoexec.ipxe`。`--script-dir` 控制脚本放在
//! ESP 根目录（`root`）、与 iPXE 同目录（`efi`）或两处都放（`both`，默认），用于验证 H5。

use serde_json::json;
use xpxe_image::Timestamp;
use xpxe_image::fat::{self, FileSpec};
use xpxe_image::iso::{self, Entry};

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
