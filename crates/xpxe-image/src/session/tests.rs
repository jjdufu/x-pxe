//! 会话 ISO：读回结构、确定性、卷标规则。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{AUTOEXEC_PATH, BootIsoInput, EFI_PATH, ESP_NAME, build_boot_iso, volume_label};
use crate::Timestamp;
use crate::readback::{read_fat, read_iso};

/// 会话创建时间。
const CREATED: Timestamp = Timestamp {
    year: 2026,
    month: 11,
    day: 18,
    hour: 9,
    minute: 41,
    second: 7,
};

/// 构造一份类似 iPXE 的输入（约 1 MiB 伪 EFI）。
///
/// # Errors
///
/// 生成失败时返回错误。
fn sample(session: &str, script: &[u8]) -> TestResult<Vec<u8>> {
    let efi: Vec<u8> = (0..1_000_000_u32)
        .map(|index| index.to_le_bytes().first().copied().unwrap_or_default())
        .collect();
    Ok(build_boot_iso(&BootIsoInput {
        session_id: session,
        created: CREATED,
        ipxe_efi: &efi,
        autoexec: script,
    })?)
}

/// 读回：卷标、只有 ESP 一个文件、El Torito 只有一项（0xEF、no-emulation、指向 ESP），ESP 内两份文件字节一致。
///
/// # Errors
///
/// 结构不符时返回错误。
#[test]
fn readback_structure() -> TestResult {
    let script = b"#!ipxe\nchain http://x/p/t/boot.ipxe\n";
    let image = sample("3f2a9c1e-77aa-4bcd-9e00-000000000001", script)?;
    ensure(image.len() < 5 * 1024 * 1024, "ISO 小于 5 MB")?;
    let view = read_iso(&image)?;
    ensure_eq(&view.volume_id.as_str(), &"XPXE-3F2A9C1E", "卷标")?;
    ensure_eq(&view.files.len(), &1, "根目录只有 ESP")?;
    let esp = view.file(ESP_NAME).ok_or("缺少 ESP.IMG")?;
    ensure_eq(&view.boot_entries.len(), &1, "El Torito 只有一项")?;
    let boot = view.boot_entries.first().ok_or("无引导项")?;
    ensure_eq(&boot.platform, &0xEF, "平台 UEFI")?;
    ensure(boot.bootable && boot.no_emulation, "可引导且 no-emulation")?;
    let esp_offset = usize::try_from(boot.load_lba)?
        .checked_mul(2048)
        .ok_or("偏移溢出")?;
    ensure_eq(
        &image.get(esp_offset..esp_offset.checked_add(esp.data.len()).ok_or("溢出")?),
        &Some(esp.data.as_slice()),
        "引导项指向 ESP",
    )?;
    let fat = read_fat(&esp.data)?;
    ensure_eq(&fat.files.len(), &2, "ESP 内两份文件")?;
    ensure_eq(
        &fat.file(EFI_PATH).map(|file| file.data.len()),
        &Some(1_000_000),
        "BOOTX64.EFI 大小",
    )?;
    ensure_eq(
        &fat.file(AUTOEXEC_PATH).map(|file| file.data.as_slice()),
        &Some(script.as_slice()),
        "autoexec.ipxe 字节一致",
    )?;
    ensure(
        fat.files.iter().any(|file| file.path == AUTOEXEC_PATH),
        "长文件名保留大小写",
    )?;
    Ok(())
}

/// 同一输入两次生成字节一致；脚本不同则不同。
///
/// # Errors
///
/// 不确定时返回错误。
#[test]
fn deterministic() -> TestResult {
    let first = sample("abcdef0123456789", b"#!ipxe\n")?;
    let second = sample("abcdef0123456789", b"#!ipxe\n")?;
    ensure(first == second, "同输入同字节")?;
    let other = sample("abcdef0123456789", b"#!ipxe\necho\n")?;
    ensure(first != other, "脚本不同则 ISO 不同")?;
    Ok(())
}

/// 卷标取前 8 个字母数字并大写；不足 8 个拒绝。
///
/// # Errors
///
/// 规则不符时返回错误。
#[test]
fn label_rules() -> TestResult {
    ensure_eq(
        &volume_label("ab-cd-ef-12-34")?.as_str(),
        &"XPXE-ABCDEF12",
        "去掉连字符",
    )?;
    ensure(volume_label("short-1").is_err(), "不足 8 位")?;
    Ok(())
}
