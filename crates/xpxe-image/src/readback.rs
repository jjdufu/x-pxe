//! 读回解析器：独立于写出器重新解析 ISO9660 / El Torito 与 FAT12/16，供测试与验收核对生成结果。
//!
//! 只依赖格式规范本身（不复用写出器的常量与布局假设），因此能发现写出器的布局错误。

use crate::ImageError;
use crate::math::{add, mul};

/// ISO9660 逻辑扇区大小。
const ISO_SECTOR: usize = 2048;
/// 目录嵌套上限（防止恶意镜像造成无限递归）。
const MAX_DEPTH: usize = 16;

/// 读回的 ISO 文件（只列根目录下的普通文件）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoFile {
    /// 去掉 `;1` 版本号后的文件名。
    pub name: String,
    /// 文件内容。
    pub data: Vec<u8>,
}

/// El Torito 引导项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootEntry {
    /// 平台 ID（0x00 BIOS、0xEF UEFI）。
    pub platform: u8,
    /// 是否可引导（0x88）。
    pub bootable: bool,
    /// 介质类型 0 = no-emulation。
    pub no_emulation: bool,
    /// 映像起始扇区（2048 字节扇区）。
    pub load_lba: u32,
    /// 虚拟扇区数（512 字节）。
    pub sector_count: u16,
}

/// 读回的 ISO 结构。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoView {
    /// 卷标（去掉尾部空格）。
    pub volume_id: String,
    /// 根目录文件。
    pub files: Vec<IsoFile>,
    /// El Torito 引导项（按目录顺序）。
    pub boot_entries: Vec<BootEntry>,
}

impl IsoView {
    /// 按名字取根目录文件。
    #[must_use]
    pub fn file(&self, name: &str) -> Option<&IsoFile> {
        self.files.iter().find(|file| file.name == name)
    }
}

/// 读回的 FAT 文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatFile {
    /// 以 `/` 分隔的完整路径（优先长文件名）。
    pub path: String,
    /// 文件内容。
    pub data: Vec<u8>,
}

/// 读回的 FAT 结构。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatView {
    /// FAT 位宽：12 或 16。
    pub bits: u8,
    /// 全部普通文件。
    pub files: Vec<FatFile>,
}

impl FatView {
    /// 按路径取文件（大小写不敏感，FAT 语义）。
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&FatFile> {
        self.files
            .iter()
            .find(|file| file.path.eq_ignore_ascii_case(path))
    }
}

/// 结构错误。
fn bad(what: &str) -> ImageError {
    ImageError::Malformed(what.to_owned())
}

/// 取 `buf[offset..offset + len]`。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn slice(buf: &[u8], offset: usize, len: usize) -> Result<&[u8], ImageError> {
    buf.get(offset..add(offset, len)?)
        .ok_or_else(|| bad("越界读取"))
}

/// 读一个字节。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn byte(buf: &[u8], offset: usize) -> Result<u8, ImageError> {
    buf.get(offset).copied().ok_or_else(|| bad("越界读取"))
}

/// 小端 u16。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn le16(buf: &[u8], offset: usize) -> Result<u16, ImageError> {
    let raw = slice(buf, offset, 2)?;
    Ok(u16::from_le_bytes([byte(raw, 0)?, byte(raw, 1)?]))
}

/// 小端 u32。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn le32(buf: &[u8], offset: usize) -> Result<u32, ImageError> {
    let raw = slice(buf, offset, 4)?;
    Ok(u32::from_le_bytes([
        byte(raw, 0)?,
        byte(raw, 1)?,
        byte(raw, 2)?,
        byte(raw, 3)?,
    ]))
}

/// `u32` → `usize`。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn width(value: u32) -> Result<usize, ImageError> {
    usize::try_from(value).map_err(|_error| bad("数值超出平台范围"))
}

/// 解析 ISO9660 + El Torito。
///
/// # Errors
///
/// 缺少主卷描述符、目录记录越界或引导目录校验失败时返回 [`ImageError::Malformed`]。
pub fn read_iso(image: &[u8]) -> Result<IsoView, ImageError> {
    let mut pvd = None;
    let mut catalog_lba = None;
    for sector in 16..64_usize {
        let desc = slice(image, mul(sector, ISO_SECTOR)?, ISO_SECTOR)?;
        if slice(desc, 1, 5)? != b"CD001" {
            return Err(bad("卷描述符签名错误"));
        }
        match byte(desc, 0)? {
            1 => pvd = Some(desc),
            0 if slice(desc, 7, 23)? == b"EL TORITO SPECIFICATION" => {
                catalog_lba = Some(width(le32(desc, 0x47)?)?);
            }
            255 => break,
            _other => {}
        }
    }
    let primary = pvd.ok_or_else(|| bad("缺少主卷描述符"))?;
    let volume_id = String::from_utf8_lossy(slice(primary, 40, 32)?)
        .trim_end()
        .to_owned();
    let root = slice(primary, 156, 34)?;
    let files = read_root(image, width(le32(root, 2)?)?, width(le32(root, 10)?)?)?;
    let boot_entries = match catalog_lba {
        Some(lba) => read_catalog(slice(image, mul(lba, ISO_SECTOR)?, ISO_SECTOR)?)?,
        None => Vec::new(),
    };
    Ok(IsoView {
        volume_id,
        files,
        boot_entries,
    })
}

/// 读根目录中的普通文件。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn read_root(image: &[u8], lba: usize, len: usize) -> Result<Vec<IsoFile>, ImageError> {
    let dir = slice(image, mul(lba, ISO_SECTOR)?, len)?;
    let mut files = Vec::new();
    let mut pos = 0_usize;
    while pos < dir.len() {
        let rec_len = usize::from(byte(dir, pos)?);
        if rec_len == 0 {
            // 记录不跨扇区：跳到下一扇区开头
            let next = pos
                .checked_div(ISO_SECTOR)
                .and_then(|index| index.checked_add(1))
                .ok_or_else(|| bad("目录偏移溢出"))?;
            pos = mul(next, ISO_SECTOR)?;
            continue;
        }
        let rec = slice(dir, pos, rec_len)?;
        let name_len = usize::from(byte(rec, 32)?);
        let raw_name = slice(rec, 33, name_len)?;
        let is_dir = byte(rec, 25)? & 0x02 != 0;
        if !is_dir && raw_name != [0] && raw_name != [1] {
            let text = String::from_utf8_lossy(raw_name);
            let name = text.split(';').next().unwrap_or_default().to_owned();
            let data = slice(
                image,
                mul(width(le32(rec, 2)?)?, ISO_SECTOR)?,
                width(le32(rec, 10)?)?,
            )?
            .to_vec();
            files.push(IsoFile { name, data });
        }
        pos = add(pos, rec_len)?;
    }
    Ok(files)
}

/// 解析引导目录：验证项、默认项与后续节。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn read_catalog(catalog: &[u8]) -> Result<Vec<BootEntry>, ImageError> {
    let validation = slice(catalog, 0, 32)?;
    if byte(validation, 0)? != 1 || slice(validation, 30, 2)? != [0x55, 0xAA] {
        return Err(bad("引导目录验证项无效"));
    }
    let mut sum = 0_u16;
    for index in 0..16_usize {
        sum = sum.wrapping_add(le16(validation, mul(index, 2)?)?);
    }
    if sum != 0 {
        return Err(bad("引导目录校验和不为零"));
    }
    let mut entries = vec![entry(byte(validation, 1)?, slice(catalog, 32, 32)?)?];
    let mut pos = 64_usize;
    while pos < catalog.len() {
        let header = slice(catalog, pos, 32)?;
        let kind = byte(header, 0)?;
        if kind != 0x90 && kind != 0x91 {
            break;
        }
        let platform = byte(header, 1)?;
        let count = usize::from(le16(header, 2)?);
        pos = add(pos, 32)?;
        for _index in 0..count {
            entries.push(entry(platform, slice(catalog, pos, 32)?)?);
            pos = add(pos, 32)?;
        }
        if kind == 0x91 {
            break;
        }
    }
    Ok(entries)
}

/// 解析一个 32 字节引导项。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn entry(platform: u8, raw: &[u8]) -> Result<BootEntry, ImageError> {
    Ok(BootEntry {
        platform,
        bootable: byte(raw, 0)? == 0x88,
        no_emulation: byte(raw, 1)? == 0,
        load_lba: le32(raw, 8)?,
        sector_count: le16(raw, 6)?,
    })
}

/// FAT 几何参数。
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// 每簇字节数。
    cluster_bytes: usize,
    /// FAT 表起始偏移。
    fat_offset: usize,
    /// 根目录起始偏移。
    root_offset: usize,
    /// 根目录字节数。
    root_bytes: usize,
    /// 数据区起始偏移。
    data_offset: usize,
    /// 数据簇总数。
    clusters: usize,
    /// FAT 位宽。
    bits: u8,
}

/// 目录项。
#[derive(Debug, Clone)]
struct DirEntry {
    /// 名字（长名优先）。
    name: String,
    /// 是否目录。
    is_dir: bool,
    /// 起始簇。
    cluster: usize,
    /// 字节数。
    size: usize,
}

/// 解析 FAT12/16 文件系统。
///
/// # Errors
///
/// 引导扇区参数非法、FAT32、簇链越界或成环时返回 [`ImageError::Malformed`]。
pub fn read_fat(image: &[u8]) -> Result<FatView, ImageError> {
    let geo = geometry(image)?;
    let mut files = Vec::new();
    let root = parse_dir(slice(image, geo.root_offset, geo.root_bytes)?)?;
    walk((image, &geo), root, "", 0, &mut files)?;
    Ok(FatView {
        bits: geo.bits,
        files,
    })
}

/// 读引导扇区参数。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn geometry(image: &[u8]) -> Result<Geometry, ImageError> {
    if slice(image, 510, 2)? != [0x55, 0xAA] {
        return Err(bad("引导扇区缺少 55AA"));
    }
    let bps = usize::from(le16(image, 11)?);
    let spc = usize::from(byte(image, 13)?);
    let reserved = usize::from(le16(image, 14)?);
    let fats = usize::from(byte(image, 16)?);
    let root_entries = usize::from(le16(image, 17)?);
    let total16 = le16(image, 19)?;
    let fat_size = usize::from(le16(image, 22)?);
    if bps == 0 || spc == 0 || fat_size == 0 || root_entries == 0 {
        return Err(bad("BPB 参数为零（或为 FAT32）"));
    }
    let total = if total16 == 0 {
        width(le32(image, 32)?)?
    } else {
        usize::from(total16)
    };
    let root_bytes = mul(root_entries, 32)?;
    let root_sectors = add(root_bytes, bps.saturating_sub(1))?
        .checked_div(bps)
        .ok_or_else(|| bad("扇区大小为零"))?;
    let fat_offset = mul(reserved, bps)?;
    let root_offset = add(fat_offset, mul(mul(fats, fat_size)?, bps)?)?;
    let first_data = add(add(reserved, mul(fats, fat_size)?)?, root_sectors)?;
    let clusters = total
        .checked_sub(first_data)
        .and_then(|sectors| sectors.checked_div(spc))
        .ok_or_else(|| bad("总扇区数小于数据区起点"))?;
    let bits = if clusters < 4085 {
        12
    } else if clusters < 65525 {
        16
    } else {
        return Err(bad("簇数超出 FAT16（不支持 FAT32）"));
    };
    Ok(Geometry {
        cluster_bytes: mul(bps, spc)?,
        fat_offset,
        root_offset,
        root_bytes,
        data_offset: mul(first_data, bps)?,
        clusters,
        bits,
    })
}

/// 下一簇；`None` 表示链结束。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn next_cluster(image: &[u8], geo: &Geometry, cluster: usize) -> Result<Option<usize>, ImageError> {
    let (value, end) = if geo.bits == 12 {
        let half = cluster.checked_div(2).ok_or_else(|| bad("簇号溢出"))?;
        let raw = le16(image, add(geo.fat_offset, add(cluster, half)?)?)?;
        let value = if cluster & 1 == 1 {
            raw >> 4_u32
        } else {
            raw & 0x0FFF
        };
        (usize::from(value), 0x0FF8)
    } else {
        (
            usize::from(le16(image, add(geo.fat_offset, mul(cluster, 2)?)?)?),
            0xFFF8,
        )
    };
    if value >= end {
        return Ok(None);
    }
    if value < 2 || value >= add(geo.clusters, 2)? {
        return Err(bad("簇链指向非法簇"));
    }
    Ok(Some(value))
}

/// 读整条簇链；`size` 给出时截断到该长度。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn read_chain(
    image: &[u8],
    geo: &Geometry,
    start: usize,
    size: Option<usize>,
) -> Result<Vec<u8>, ImageError> {
    let mut out = Vec::new();
    if start == 0 {
        return Ok(out);
    }
    let mut cluster = Some(start);
    let mut hops = 0_usize;
    while let Some(current) = cluster {
        hops = add(hops, 1)?;
        if hops > add(geo.clusters, 2)? {
            return Err(bad("簇链成环"));
        }
        let index = current.checked_sub(2).ok_or_else(|| bad("簇号小于 2"))?;
        let offset = add(geo.data_offset, mul(index, geo.cluster_bytes)?)?;
        out.extend_from_slice(slice(image, offset, geo.cluster_bytes)?);
        if size.is_some_and(|limit| out.len() >= limit) {
            break;
        }
        cluster = next_cluster(image, geo, current)?;
    }
    if let Some(limit) = size {
        if out.len() < limit {
            return Err(bad("簇链短于文件大小"));
        }
        out.truncate(limit);
    }
    Ok(out)
}

/// 递归遍历目录。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn walk(
    (image, geo): (&[u8], &Geometry),
    entries: Vec<DirEntry>,
    prefix: &str,
    depth: usize,
    files: &mut Vec<FatFile>,
) -> Result<(), ImageError> {
    if depth > MAX_DEPTH {
        return Err(bad("目录嵌套过深"));
    }
    for item in entries {
        let path = if prefix.is_empty() {
            item.name.clone()
        } else {
            format!("{prefix}/{}", item.name)
        };
        if item.is_dir {
            let raw = read_chain(image, geo, item.cluster, None)?;
            walk((image, geo), parse_dir(&raw)?, &path, add(depth, 1)?, files)?;
        } else {
            let data = read_chain(image, geo, item.cluster, Some(item.size))?;
            files.push(FatFile { path, data });
        }
    }
    Ok(())
}

/// 解析目录区（含长文件名）。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn parse_dir(raw: &[u8]) -> Result<Vec<DirEntry>, ImageError> {
    let mut out = Vec::new();
    let mut lfn: Vec<(u8, Vec<u16>)> = Vec::new();
    for block in raw.as_chunks::<32>().0 {
        let chunk = block.as_slice();
        let first = byte(chunk, 0)?;
        if first == 0 {
            break;
        }
        let attr = byte(chunk, 11)?;
        if first == 0xE5 {
            lfn.clear();
            continue;
        }
        if attr == 0x0F {
            lfn.push((first & 0x1F, lfn_units(chunk)?));
            continue;
        }
        if attr & 0x08 != 0 {
            lfn.clear();
            continue;
        }
        let short = short_name(chunk)?;
        if short == "." || short == ".." {
            lfn.clear();
            continue;
        }
        let name = if lfn.is_empty() {
            short
        } else {
            lfn.sort_by_key(|part| part.0);
            let units: Vec<u16> = lfn
                .iter()
                .flat_map(|part| part.1.iter().copied())
                .take_while(|unit| *unit != 0)
                .collect();
            String::from_utf16(&units).map_err(|_error| bad("长文件名不是合法 UTF-16"))?
        };
        lfn.clear();
        out.push(DirEntry {
            name,
            is_dir: attr & 0x10 != 0,
            cluster: usize::from(le16(chunk, 26)?),
            size: width(le32(chunk, 28)?)?,
        });
    }
    Ok(out)
}

/// 长文件名项中的 13 个 UTF-16 码元（偏移 1/14/28 处的三段）。
///
/// # Errors
///
/// 越界时返回 [`ImageError::Malformed`]。
fn lfn_units(chunk: &[u8]) -> Result<Vec<u16>, ImageError> {
    let mut units = Vec::with_capacity(13);
    for offset in [1_usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30] {
        units.push(le16(chunk, offset)?);
    }
    Ok(units)
}

/// 8.3 短名（按 NT 保留字节的小写标志还原大小写）。
///
/// # Errors
///
/// 镜像结构不合法或越界时返回 [`ImageError::Malformed`]。
fn short_name(chunk: &[u8]) -> Result<String, ImageError> {
    let flags = byte(chunk, 12)?;
    let mut base = String::from_utf8_lossy(slice(chunk, 0, 8)?)
        .trim_end()
        .to_owned();
    let mut ext = String::from_utf8_lossy(slice(chunk, 8, 3)?)
        .trim_end()
        .to_owned();
    if flags & 0x08 != 0 {
        base = base.to_ascii_lowercase();
    }
    if flags & 0x10 != 0 {
        ext = ext.to_ascii_lowercase();
    }
    Ok(if ext.is_empty() {
        base
    } else {
        format!("{base}.{ext}")
    })
}

#[cfg(test)]
mod tests;
