//! ISO9660 + El Torito 写出器。
//!
//! 布局固定：系统区（16 扇区）→ 主卷描述符 → El Torito 引导记录 → 卷描述符集终止符 →
//! 引导目录 → L/M 路径表 → 根目录 → 文件数据。只有根目录一层，文件名为 ISO9660 一级名
//! （8.3 大写 + `;1`）。El Torito 引导目录只有一项：平台 0xEF（UEFI）、no-emulation，
//! 指向调用方指定的 ESP 映像文件。

use crate::math::{add, div_ceil, mul, put, to_u16, to_u32};
use crate::{ImageError, Timestamp};

/// 逻辑扇区大小。
const SECTOR: usize = 2048;
/// 主卷描述符所在扇区。
const PVD_LBA: usize = 16;
/// 引导记录所在扇区。
const BOOT_RECORD_LBA: usize = 17;
/// 终止符所在扇区。
const TERMINATOR_LBA: usize = 18;
/// 引导目录所在扇区。
const CATALOG_LBA: usize = 19;
/// L 型路径表所在扇区。
const PATH_L_LBA: usize = 20;
/// M 型路径表所在扇区。
const PATH_M_LBA: usize = 21;
/// 根目录所在扇区。
const ROOT_LBA: usize = 22;
/// 第一个文件的起始扇区。
const FIRST_FILE_LBA: usize = 23;

/// 光盘中的一个文件。
#[derive(Debug, Clone, Copy)]
pub struct Entry<'data> {
    /// ISO9660 一级名，例如 `ESP.IMG`（写入时自动追加 `;1`）。
    pub name: &'data str,
    /// 文件内容。
    pub data: &'data [u8],
}

/// 写入 ISO9660 的“双字节序”32 位整数（先小端后大端）。
const fn both32(value: u32) -> [u8; 8] {
    let [le0, le1, le2, le3] = value.to_le_bytes();
    let [be0, be1, be2, be3] = value.to_be_bytes();
    [le0, le1, le2, le3, be0, be1, be2, be3]
}

/// 写入 ISO9660 的“双字节序”16 位整数。
const fn both16(value: u16) -> [u8; 4] {
    let [le0, le1] = value.to_le_bytes();
    let [be0, be1] = value.to_be_bytes();
    [le0, le1, be0, be1]
}

/// 校验一级文件名：1–8 位主名 + 可选 1–3 位扩展名，只含大写字母、数字与下划线。
fn valid_name(name: &str) -> bool {
    let (base, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let ok = |part: &str, max: usize| {
        part.len() <= max
            && part
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    };
    !base.is_empty() && ok(base, 8) && ok(ext, 3)
}

/// 目录记录中的 7 字节时间。
fn dir_time(stamp: Timestamp) -> [u8; 7] {
    let years = u8::try_from(stamp.year.saturating_sub(1900)).unwrap_or(u8::MAX);
    [
        years,
        stamp.month,
        stamp.day,
        stamp.hour,
        stamp.minute,
        stamp.second,
        0,
    ]
}

/// 卷描述符中的 17 字节时间（`YYYYMMDDHHMMSScc` + 时区）。
fn volume_time(stamp: Timestamp) -> [u8; 17] {
    let text = format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}00",
        stamp.year, stamp.month, stamp.day, stamp.hour, stamp.minute, stamp.second
    );
    let mut out = [0_u8; 17];
    out.iter_mut()
        .zip(text.bytes())
        .for_each(|(slot, byte)| *slot = byte);
    out
}

/// 一条目录记录。
///
/// # Errors
///
/// 字段溢出时返回 [`ImageError::TooLarge`]。
fn dir_record(
    name: &[u8],
    lba: usize,
    size: usize,
    is_dir: bool,
    stamp: Timestamp,
) -> Result<Vec<u8>, ImageError> {
    let name_len = name.len();
    let base_len = add(33, name_len)?;
    let total = if base_len.is_multiple_of(2) {
        base_len
    } else {
        add(base_len, 1)?
    };
    let mut record = vec![0_u8; total];
    put(&mut record, 0, &[u8::try_from(total)?, 0])?;
    put(&mut record, 2, &both32(to_u32(lba)?))?;
    put(&mut record, 10, &both32(to_u32(size)?))?;
    put(&mut record, 18, &dir_time(stamp))?;
    put(&mut record, 25, &[if is_dir { 2 } else { 0 }, 0, 0])?;
    put(&mut record, 28, &both16(1))?;
    put(&mut record, 32, &[u8::try_from(name_len)?])?;
    put(&mut record, 33, name)?;
    Ok(record)
}

/// 填充为空格的定长文本字段。
fn padded<const N: usize>(text: &str) -> [u8; N] {
    let mut out = [b' '; N];
    out.iter_mut()
        .zip(text.bytes())
        .for_each(|(slot, byte)| *slot = byte);
    out
}

/// 生成 UEFI 引导 ISO。
///
/// `esp` 必须是 `files` 中某个文件的名字，它会成为 El Torito 唯一的引导项。
///
/// # Errors
///
/// 文件名不合法、重名、`esp` 不存在或内容超出 ISO9660 上限时返回错误。
pub fn build(
    files: &[Entry<'_>],
    esp: &str,
    volume_id: &str,
    stamp: Timestamp,
) -> Result<Vec<u8>, ImageError> {
    let checked = stamp.validate()?;
    let mut sorted: Vec<Entry<'_>> = files.to_vec();
    sorted.sort_by(|left, right| left.name.cmp(right.name));
    if sorted
        .windows(2)
        .any(|pair| matches!(pair, [left, right] if left.name == right.name))
    {
        return Err(ImageError::BadPath("重名".to_owned()));
    }
    if let Some(bad) = sorted.iter().find(|file| !valid_name(file.name)) {
        return Err(ImageError::BadPath(bad.name.to_owned()));
    }
    let mut placements = Vec::with_capacity(sorted.len());
    let mut lba = FIRST_FILE_LBA;
    for file in &sorted {
        placements.push((*file, lba));
        lba = add(lba, div_ceil(file.data.len(), SECTOR)?.max(1))?;
    }
    let total_sectors = lba;
    let esp_place = placements
        .iter()
        .find(|(file, _)| file.name == esp)
        .copied()
        .ok_or_else(|| ImageError::BadPath(esp.to_owned()))?;

    let mut image = vec![0_u8; mul(total_sectors, SECTOR)?];
    let root = root_directory(&placements, checked)?;
    put(&mut image, mul(ROOT_LBA, SECTOR)?, &root)?;
    write_pvd(&mut image, volume_id, total_sectors, checked)?;
    write_boot_record(&mut image)?;
    put(
        &mut image,
        mul(TERMINATOR_LBA, SECTOR)?,
        &[255, b'C', b'D', b'0', b'0', b'1', 1],
    )?;
    write_catalog(&mut image, esp_place.1, esp_place.0.data.len())?;
    write_path_tables(&mut image)?;
    for (file, at) in &placements {
        put(&mut image, mul(*at, SECTOR)?, file.data)?;
    }
    Ok(image)
}

/// 根目录扇区内容。
///
/// # Errors
///
/// 根目录超过一个扇区时返回 [`ImageError::TooLarge`]。
fn root_directory(
    placements: &[(Entry<'_>, usize)],
    stamp: Timestamp,
) -> Result<Vec<u8>, ImageError> {
    let mut root = dir_record(&[0], ROOT_LBA, SECTOR, true, stamp)?;
    root.extend(dir_record(&[1], ROOT_LBA, SECTOR, true, stamp)?);
    for (file, at) in placements {
        let name = format!("{};1", file.name);
        root.extend(dir_record(
            name.as_bytes(),
            *at,
            file.data.len(),
            false,
            stamp,
        )?);
    }
    if root.len() > SECTOR {
        return Err(ImageError::TooLarge);
    }
    Ok(root)
}

/// 主卷描述符。
///
/// # Errors
///
/// 字段溢出时返回 [`ImageError::TooLarge`]。
fn write_pvd(
    image: &mut [u8],
    volume_id: &str,
    total_sectors: usize,
    stamp: Timestamp,
) -> Result<(), ImageError> {
    let base = mul(PVD_LBA, SECTOR)?;
    let at = |offset: usize| add(base, offset);
    put(image, base, &[1, b'C', b'D', b'0', b'0', b'1', 1, 0])?;
    put(image, at(8)?, &padded::<32>(""))?;
    put(
        image,
        at(40)?,
        &padded::<32>(&volume_id.to_ascii_uppercase()),
    )?;
    put(image, at(80)?, &both32(to_u32(total_sectors)?))?;
    put(image, at(120)?, &both16(1))?;
    put(image, at(124)?, &both16(1))?;
    put(image, at(128)?, &both16(to_u16(SECTOR)?))?;
    put(image, at(132)?, &both32(10))?;
    put(image, at(140)?, &to_u32(PATH_L_LBA)?.to_le_bytes())?;
    put(image, at(148)?, &to_u32(PATH_M_LBA)?.to_be_bytes())?;
    put(
        image,
        at(156)?,
        &dir_record(&[0], ROOT_LBA, SECTOR, true, stamp)?,
    )?;
    put(image, at(190)?, &padded::<128>(""))?;
    put(image, at(318)?, &padded::<128>("X-PXE"))?;
    put(image, at(446)?, &padded::<128>("X-PXE"))?;
    put(image, at(574)?, &padded::<128>("X-PXE XPXE-IMAGE"))?;
    put(image, at(702)?, &padded::<37>(""))?;
    put(image, at(739)?, &padded::<37>(""))?;
    put(image, at(776)?, &padded::<37>(""))?;
    let time = volume_time(stamp);
    put(image, at(813)?, &time)?;
    put(image, at(830)?, &time)?;
    let mut never = [b'0'; 17];
    if let Some(zone) = never.last_mut() {
        *zone = 0;
    }
    put(image, at(847)?, &never)?;
    put(image, at(864)?, &time)?;
    put(image, at(881)?, &[1])
}

/// El Torito 引导记录卷描述符。
///
/// # Errors
///
/// 越界时返回 [`ImageError::TooLarge`]。
fn write_boot_record(image: &mut [u8]) -> Result<(), ImageError> {
    let base = mul(BOOT_RECORD_LBA, SECTOR)?;
    put(image, base, &[0, b'C', b'D', b'0', b'0', b'1', 1])?;
    put(image, add(base, 7)?, b"EL TORITO SPECIFICATION")?;
    put(image, add(base, 0x47)?, &to_u32(CATALOG_LBA)?.to_le_bytes())
}

/// El Torito 引导目录：验证项（平台 0xEF）+ 默认项（no-emulation，指向 ESP）。
///
/// # Errors
///
/// 越界时返回 [`ImageError::TooLarge`]。
fn write_catalog(image: &mut [u8], esp_lba: usize, esp_len: usize) -> Result<(), ImageError> {
    let mut validation = [0_u8; 32];
    put(&mut validation, 0, &[1, 0xEF])?;
    put(&mut validation, 4, b"X-PXE")?;
    put(&mut validation, 30, &[0x55, 0xAA])?;
    let sum = validation
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .fold(0_u16, |acc, pair| {
            acc.wrapping_add(u16::from_le_bytes(pair))
        });
    put(&mut validation, 28, &0_u16.wrapping_sub(sum).to_le_bytes())?;
    let virtual_sectors = u16::try_from(div_ceil(esp_len, 512)?).unwrap_or(0);
    let mut entry = [0_u8; 32];
    put(&mut entry, 0, &[0x88, 0])?;
    put(&mut entry, 6, &virtual_sectors.to_le_bytes())?;
    put(&mut entry, 8, &to_u32(esp_lba)?.to_le_bytes())?;
    let base = mul(CATALOG_LBA, SECTOR)?;
    put(image, base, &validation)?;
    put(image, add(base, 32)?, &entry)
}

/// L 型与 M 型路径表（只有根目录一项）。
///
/// # Errors
///
/// 越界时返回 [`ImageError::TooLarge`]。
fn write_path_tables(image: &mut [u8]) -> Result<(), ImageError> {
    let root = to_u32(ROOT_LBA)?;
    let mut little = [0_u8; 10];
    put(&mut little, 0, &[1, 0])?;
    put(&mut little, 2, &root.to_le_bytes())?;
    put(&mut little, 6, &1_u16.to_le_bytes())?;
    let mut big = [0_u8; 10];
    put(&mut big, 0, &[1, 0])?;
    put(&mut big, 2, &root.to_be_bytes())?;
    put(&mut big, 6, &1_u16.to_be_bytes())?;
    put(image, mul(PATH_L_LBA, SECTOR)?, &little)?;
    put(image, mul(PATH_M_LBA, SECTOR)?, &big)
}

#[cfg(test)]
mod tests;
