//! 流式 ISO 读取测试：Rock Ridge 长名与子目录（测试内构造）、写出器生成的 ISO9660 原名、initrd 打包与发行版识别。

use std::io::Cursor;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{Family, IsoEntry, IsoReader, detect};
use crate::Timestamp;
use crate::iso::{Entry, build};

/// 2048 字节扇区。
const SECTOR_LEN: usize = 2048;
/// 根目录扇区。
const ROOT: u32 = 20;
/// `/boot` 扇区。
const BOOT: u32 = 21;

/// 在 `offset` 处写入。
///
/// # Errors
///
/// 越界时返回错误。
fn put(img: &mut [u8], offset: usize, bytes: &[u8]) -> TestResult {
    let end = offset.checked_add(bytes.len()).ok_or("溢出")?;
    img.get_mut(offset..end)
        .ok_or("越界")?
        .copy_from_slice(bytes);
    Ok(())
}

/// 扇区起点。
fn at(lba: u32) -> usize {
    usize::try_from(lba).unwrap_or(0).saturating_mul(SECTOR_LEN)
}

/// 目录记录（`su` 为系统使用区）。
///
/// # Errors
///
/// 长度超限时返回错误。
fn record(name: &[u8], extent: (u32, u32), dir: bool, su: &[u8]) -> TestResult<Vec<u8>> {
    let (lba, len) = extent;
    let mut rec = vec![0_u8; 33];
    rec.extend_from_slice(name);
    if name.len().is_multiple_of(2) {
        rec.push(0);
    }
    rec.extend_from_slice(su);
    let total = u8::try_from(rec.len())?;
    put(&mut rec, 0, &[total])?;
    put(&mut rec, 2, &lba.to_le_bytes())?;
    put(&mut rec, 6, &lba.to_be_bytes())?;
    put(&mut rec, 10, &len.to_le_bytes())?;
    put(&mut rec, 14, &len.to_be_bytes())?;
    put(&mut rec, 25, &[if dir { 2 } else { 0 }])?;
    put(&mut rec, 32, &[u8::try_from(name.len())?])?;
    Ok(rec)
}

/// SUSP NM 项。
///
/// # Errors
///
/// 名字过长时返回错误。
fn nm(name: &str) -> TestResult<Vec<u8>> {
    let mut out = vec![
        b'N',
        b'M',
        u8::try_from(name.len().saturating_add(5))?,
        1,
        0,
    ];
    out.extend_from_slice(name.as_bytes());
    Ok(out)
}

/// 构造带 Rock Ridge 的小 ISO：/boot/linux26、/boot/initrd.img、/README.txt。
///
/// # Errors
///
/// 构造失败时返回错误。
fn rr_iso() -> TestResult<Vec<u8>> {
    let mut img = vec![0_u8; 25 * SECTOR_LEN];
    let pvd = 16 * SECTOR_LEN;
    put(&mut img, pvd, b"\x01CD001")?;
    put(
        &mut img,
        pvd.saturating_add(40),
        b"PVE                             ",
    )?;
    let root_dot = record(&[0], (ROOT, 2048), true, &[b'S', b'P', 7, 1, 0xBE, 0xEF, 0])?;
    put(
        &mut img,
        pvd.saturating_add(156),
        root_dot.get(..34).ok_or("短")?,
    )?;
    put(&mut img, 17 * SECTOR_LEN, b"\xFFCD001")?;
    let mut dir = root_dot;
    dir.extend(record(&[1], (ROOT, 2048), true, &[])?);
    dir.extend(record(b"BOOT", (BOOT, 2048), true, &nm("boot")?)?);
    dir.extend(record(b"README.TXT;1", (24, 6), false, &nm("README.txt")?)?);
    put(&mut img, at(ROOT), &dir)?;
    let mut sub = record(&[0], (BOOT, 2048), true, &[])?;
    sub.extend(record(&[1], (ROOT, 2048), true, &[])?);
    sub.extend(record(b"LINUX26.;1", (22, 4), false, &nm("linux26")?)?);
    sub.extend(record(b"INITRD.IMG;1", (23, 3), false, &nm("initrd.img")?)?);
    put(&mut img, at(BOOT), &sub)?;
    put(&mut img, at(22), b"KERN")?;
    put(&mut img, at(23), b"INI")?;
    put(&mut img, at(24), b"hello\n")?;
    Ok(img)
}

/// Rock Ridge 长名与子目录：遍历、查找、拷贝与 PVE 识别。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn rock_ridge_tree() -> TestResult {
    let mut reader = IsoReader::open(Cursor::new(rr_iso()?))?;
    ensure(reader.info().names.rock_ridge, "识别 Rock Ridge")?;
    ensure(!reader.info().boot.hybrid, "不是 hybrid")?;
    ensure_eq(&reader.info().volume_id.as_str(), &"PVE", "卷标")?;
    let entries = reader.walk()?;
    let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
    ensure_eq(
        &paths,
        &vec!["/README.txt", "/boot", "/boot/initrd.img", "/boot/linux26"],
        "路径",
    )?;
    let kernel = reader.find("/boot/linux26")?.ok_or("缺少内核")?;
    ensure_eq(
        &reader.read_small(&kernel, 16)?,
        &b"KERN".to_vec(),
        "内核内容",
    )?;
    ensure(reader.find("/boot/missing")?.is_none(), "不存在的路径")?;
    let detection = detect(&entries);
    ensure_eq(&detection.family, &Family::Pve, "PVE 系")?;
    ensure(detection.embed_iso, "PVE 推荐 embed_iso")?;
    ensure_eq(
        &detection.iso_name.as_deref(),
        &Some("/proxmox.iso"),
        "ISO 文件名",
    )?;
    let initrd = reader.find("/boot/initrd.img")?.ok_or("缺少 initrd")?;
    let mut out = Vec::new();
    let written = reader.write_initrd_with_iso(&initrd, "/proxmox.iso", &mut out)?;
    ensure_eq(&u64::try_from(out.len())?, &written, "返回值等于写出字节数")?;
    ensure(
        out.starts_with(b"INI\x00070701"),
        "原 initrd + 对齐 + newc 头",
    )?;
    ensure(
        out.windows(12).any(|window| window == b"proxmox.iso\0"),
        "ISO 文件名",
    )?;
    ensure(
        out.windows(10).any(|window| window == b"TRAILER!!!"),
        "结束标记",
    )?;
    ensure(out.len().is_multiple_of(4), "整体 4 字节对齐")?;
    Ok(())
}

/// Step 9 写出器生成的 ISO（ISO9660 原名、只有根目录）：忽略大小写查找。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn plain_names_from_writer() -> TestResult {
    let stamp = Timestamp {
        year: 2026,
        month: 11,
        day: 19,
        hour: 0,
        minute: 0,
        second: 0,
    };
    let data = [7_u8; 4096];
    let image = build(
        &[Entry {
            name: "ESP.IMG",
            data: &data,
        }],
        "ESP.IMG",
        "XPXE-TEST",
        stamp,
    )?;
    let mut reader = IsoReader::open(Cursor::new(image))?;
    ensure(reader.info().boot.el_torito, "El Torito")?;
    ensure(!reader.info().names.rock_ridge, "无 Rock Ridge")?;
    let entries = reader.walk()?;
    ensure_eq(&entries.len(), &1, "一个文件")?;
    let esp = reader.find("/esp.img")?.ok_or("忽略大小写查找")?;
    ensure_eq(&esp.size, &4096, "大小")?;
    ensure_eq(&detect(&entries).family, &Family::Unknown, "未识别")?;
    Ok(())
}

/// 损坏的镜像只报错不 panic：签名错误、目录环、目录越界、数据越界。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn malformed_inputs() -> TestResult {
    ensure(
        IsoReader::open(Cursor::new(vec![0_u8; 40 * SECTOR_LEN])).is_err(),
        "无卷描述符",
    )?;
    let mut looped = rr_iso()?;
    // 根目录第 3 条记录是 BOOT：把它的扇区号改成根目录，形成目录环
    let name_at = looped
        .windows(4)
        .position(|window| window == b"BOOT")
        .ok_or("定位 BOOT 记录")?;
    let boot_rec = name_at.checked_sub(33).ok_or("偏移")?;
    put(&mut looped, boot_rec.saturating_add(2), &ROOT.to_le_bytes())?;
    let mut reader = IsoReader::open(Cursor::new(looped))?;
    ensure(reader.walk().is_err(), "目录环被拒绝")?;
    let mut truncated = rr_iso()?;
    truncated.truncate(21 * SECTOR_LEN);
    let mut short = IsoReader::open(Cursor::new(truncated))?;
    ensure(short.walk().is_err(), "目录越界被拒绝")?;
    let entry = IsoEntry {
        path: "/x".to_owned(),
        is_dir: false,
        size: 1,
        extents: vec![(999, 1)],
    };
    ensure(
        short.copy_to(&entry, &mut Vec::new()).is_err(),
        "数据越界被拒绝",
    )?;
    Ok(())
}
