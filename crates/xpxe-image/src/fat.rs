//! FAT12/16 镜像写出器。
//!
//! 只写不读：一次性给出全部文件，生成一个紧凑的、确定性的镜像。目录结构由文件路径推导，
//! 名字不符合 8.3 规则时写 VFAT 长文件名条目，短名按 `BASIS~N.EXT` 生成。

use std::collections::BTreeMap;

use crate::math::{add, div_ceil, mul, put, to_u16, to_u32};
use crate::{ImageError, Timestamp};

/// 扇区大小。
const SECTOR: usize = 512;
/// 根目录条目数（FAT12/16 根目录定长）。
const ROOT_ENTRIES: usize = 512;
/// 目录条目大小。
const ENTRY: usize = 32;
/// FAT12 能表示的最大簇数。
const FAT12_MAX_CLUSTERS: usize = 4084;
/// FAT16 的最小簇数。
const FAT16_MIN_CLUSTERS: usize = 4085;
/// FAT16 能表示的最大簇数。
const FAT16_MAX_CLUSTERS: usize = 65524;
/// 预留的空闲簇，便于固件在镜像上写入（例如 EFI 变量缓存）。
const SLACK_CLUSTERS: usize = 16;

/// 目录属性。
const ATTR_DIRECTORY: u8 = 0x10;
/// 归档属性（普通文件）。
const ATTR_ARCHIVE: u8 = 0x20;
/// 卷标属性。
const ATTR_VOLUME: u8 = 0x08;
/// 长文件名条目属性。
const ATTR_LFN: u8 = 0x0F;

/// FAT 变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// 12 位表项。
    Fat12,
    /// 16 位表项。
    Fat16,
}

/// 目录树节点。
#[derive(Debug, Default)]
struct Dir {
    /// 子目录，按名字排序。
    dirs: BTreeMap<String, Self>,
    /// 文件，按名字排序。
    files: BTreeMap<String, Vec<u8>>,
}

/// 要写入镜像的一个文件。
#[derive(Debug, Clone, Copy)]
pub struct FileSpec<'data> {
    /// 以 `/` 分隔的路径，例如 `EFI/BOOT/BOOTX64.EFI`。
    pub path: &'data str,
    /// 文件内容。
    pub data: &'data [u8],
}

/// 生成的镜像与选中的变体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// 镜像字节，长度是扇区大小的整数倍。
    pub bytes: Vec<u8>,
    /// FAT12 或 FAT16。
    pub variant: Variant,
}

/// 布局参数。
#[derive(Debug, Clone, Copy)]
struct Layout {
    /// 变体。
    variant: Variant,
    /// 每簇扇区数。
    sectors_per_cluster: usize,
    /// 簇数。
    clusters: usize,
    /// 每份 FAT 的扇区数。
    fat_sectors: usize,
}

impl Layout {
    /// 每簇字节数。
    const fn cluster_bytes(self) -> usize {
        self.sectors_per_cluster.saturating_mul(SECTOR)
    }

    /// 根目录起始字节。
    ///
    /// # Errors
    ///
    /// 溢出时返回 [`ImageError::TooLarge`]。
    fn root_offset(self) -> Result<usize, ImageError> {
        mul(add(1, mul(2, self.fat_sectors)?)?, SECTOR)
    }

    /// 数据区起始字节。
    ///
    /// # Errors
    ///
    /// 溢出时返回 [`ImageError::TooLarge`]。
    fn data_offset(self) -> Result<usize, ImageError> {
        add(self.root_offset()?, mul(ROOT_ENTRIES, ENTRY)?)
    }

    /// 镜像总扇区数。
    ///
    /// # Errors
    ///
    /// 溢出时返回 [`ImageError::TooLarge`]。
    fn total_sectors(self) -> Result<usize, ImageError> {
        add(
            add(1, mul(2, self.fat_sectors)?)?,
            add(
                div_ceil(mul(ROOT_ENTRIES, ENTRY)?, SECTOR)?,
                mul(self.clusters, self.sectors_per_cluster)?,
            )?,
        )
    }
}

/// 生成 FAT 镜像。
///
/// # Errors
///
/// 路径非法、重名或内容超过 FAT16 上限时返回错误。
pub fn build(files: &[FileSpec<'_>], label: &str, stamp: Timestamp) -> Result<Image, ImageError> {
    let checked = stamp.validate()?;
    let mut root = Dir::default();
    for spec in files {
        insert(&mut root, spec)?;
    }
    let layout = choose_layout(&root)?;
    let mut image = vec![0_u8; mul(layout.total_sectors()?, SECTOR)?];
    write_boot_sector(&mut image, layout, label)?;
    let mut writer = Writer {
        image,
        layout,
        next_cluster: 2,
        fat: vec![0_u32; add(layout.clusters, 2)?],
        stamp: checked,
    };
    writer.write_root(&root, label)?;
    writer.finish_fat()
}

/// 把一个文件插入目录树。
///
/// # Errors
///
/// 路径为空、含非法字符或重名时返回 [`ImageError::BadPath`]。
fn insert(root: &mut Dir, spec: &FileSpec<'_>) -> Result<(), ImageError> {
    let parts: Vec<&str> = spec.path.split('/').collect();
    let Some((name, parents)) = parts.split_last() else {
        return Err(ImageError::BadPath(spec.path.to_owned()));
    };
    let bad = |part: &str| {
        part.is_empty()
            || part
                .chars()
                .any(|ch| ch.is_control() || "\\:*?\"<>|".contains(ch))
    };
    if bad(name) || parents.iter().any(|part| bad(part)) {
        return Err(ImageError::BadPath(spec.path.to_owned()));
    }
    let mut dir = root;
    for part in parents {
        if dir.files.contains_key(*part) {
            return Err(ImageError::BadPath(spec.path.to_owned()));
        }
        dir = dir.dirs.entry((*part).to_owned()).or_default();
    }
    if dir.dirs.contains_key(*name)
        || dir
            .files
            .insert((*name).to_owned(), spec.data.to_vec())
            .is_some()
    {
        return Err(ImageError::BadPath(spec.path.to_owned()));
    }
    Ok(())
}

/// 一个名字占用的目录条目数（长文件名条目 + 短名条目）。
///
/// # Errors
///
/// 溢出时返回 [`ImageError::TooLarge`]。
fn entries_for(name: &str) -> Result<usize, ImageError> {
    if short_name_exact(name).is_some() {
        Ok(1)
    } else {
        add(div_ceil(name.encode_utf16().count(), 13)?, 1)
    }
}

/// 子目录（不含根）占用的字节数：`.`、`..` 与全部子项。
///
/// # Errors
///
/// 溢出时返回 [`ImageError::TooLarge`]。
fn dir_bytes(dir: &Dir) -> Result<usize, ImageError> {
    let mut entries = 2_usize;
    for name in dir.dirs.keys().chain(dir.files.keys()) {
        entries = add(entries, entries_for(name)?)?;
    }
    mul(entries, ENTRY)
}

/// 根目录的条目数（含卷标）。
///
/// # Errors
///
/// 溢出时返回 [`ImageError::TooLarge`]。
fn root_entries(dir: &Dir) -> Result<usize, ImageError> {
    let mut entries = 1_usize;
    for name in dir.dirs.keys().chain(dir.files.keys()) {
        entries = add(entries, entries_for(name)?)?;
    }
    Ok(entries)
}

/// 在给定簇大小下所需的簇数。
///
/// # Errors
///
/// 溢出时返回 [`ImageError::TooLarge`]。
fn clusters_needed(dir: &Dir, cluster_bytes: usize, is_root: bool) -> Result<usize, ImageError> {
    let mut total = if is_root {
        0
    } else {
        div_ceil(dir_bytes(dir)?, cluster_bytes)?
    };
    for data in dir.files.values() {
        total = add(total, div_ceil(data.len(), cluster_bytes)?)?;
    }
    for sub in dir.dirs.values() {
        total = add(total, clusters_needed(sub, cluster_bytes, false)?)?;
    }
    Ok(total)
}

/// 选择变体、簇大小与 FAT 大小。
///
/// # Errors
///
/// 根目录条目超过 512 或内容超过 FAT16 上限时返回 [`ImageError::TooLarge`]。
fn choose_layout(root: &Dir) -> Result<Layout, ImageError> {
    if root_entries(root)? > ROOT_ENTRIES {
        return Err(ImageError::TooLarge);
    }
    let candidates = [1_usize, 2, 4, 8, 16, 32, 64];
    for sectors_per_cluster in candidates {
        let needed = add(
            clusters_needed(root, mul(sectors_per_cluster, SECTOR)?, true)?,
            SLACK_CLUSTERS,
        )?;
        if needed <= FAT12_MAX_CLUSTERS {
            let fat_bytes = div_ceil(mul(add(needed, 2)?, 3)?, 2)?;
            return Ok(Layout {
                variant: Variant::Fat12,
                sectors_per_cluster,
                clusters: needed,
                fat_sectors: div_ceil(fat_bytes, SECTOR)?,
            });
        }
    }
    for sectors_per_cluster in candidates {
        let needed = add(
            clusters_needed(root, mul(sectors_per_cluster, SECTOR)?, true)?,
            SLACK_CLUSTERS,
        )?;
        if needed <= FAT16_MAX_CLUSTERS {
            let clusters = needed.max(FAT16_MIN_CLUSTERS);
            return Ok(Layout {
                variant: Variant::Fat16,
                sectors_per_cluster,
                clusters,
                fat_sectors: div_ceil(mul(add(clusters, 2)?, 2)?, SECTOR)?,
            });
        }
    }
    Err(ImageError::TooLarge)
}

/// 把卷标规整为 11 字节（大写、空格填充）。
fn label_bytes(label: &str) -> [u8; 11] {
    let mut out = [b' '; 11];
    out.iter_mut()
        .zip(
            label
                .bytes()
                .filter(u8::is_ascii_graphic)
                .map(|byte| byte.to_ascii_uppercase()),
        )
        .for_each(|(slot, byte)| *slot = byte);
    out
}

/// 写引导扇区（BPB）。
///
/// # Errors
///
/// 字段溢出时返回 [`ImageError::TooLarge`]。
fn write_boot_sector(image: &mut [u8], layout: Layout, label: &str) -> Result<(), ImageError> {
    let total = layout.total_sectors()?;
    put(image, 0, &[0xEB, 0x3C, 0x90])?;
    put(image, 3, b"XPXE    ")?;
    put(image, 11, &to_u16(SECTOR)?.to_le_bytes())?;
    put(image, 13, &[u8::try_from(layout.sectors_per_cluster)?])?;
    put(image, 14, &1_u16.to_le_bytes())?;
    put(image, 16, &[2])?;
    put(image, 17, &to_u16(ROOT_ENTRIES)?.to_le_bytes())?;
    if let Ok(small) = u16::try_from(total) {
        put(image, 19, &small.to_le_bytes())?;
    } else {
        put(image, 32, &to_u32(total)?.to_le_bytes())?;
    }
    put(image, 21, &[0xF8])?;
    put(image, 22, &to_u16(layout.fat_sectors)?.to_le_bytes())?;
    put(image, 24, &32_u16.to_le_bytes())?;
    put(image, 26, &64_u16.to_le_bytes())?;
    put(image, 36, &[0x80, 0, 0x29])?;
    put(image, 39, &0x5850_5845_u32.to_le_bytes())?;
    put(image, 43, &label_bytes(label))?;
    let fs_type: &[u8; 8] = match layout.variant {
        Variant::Fat12 => b"FAT12   ",
        Variant::Fat16 => b"FAT16   ",
    };
    put(image, 54, fs_type)?;
    put(image, 510, &[0x55, 0xAA])
}

/// 名字恰好是合法的大写 8.3 短名时返回 11 字节短名。
fn short_name_exact(name: &str) -> Option<[u8; 11]> {
    let (base, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let allowed = |byte: u8| {
        byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$%'-_@~`!(){}^#&".contains(&byte)
    };
    let valid = |part: &str, max: usize| part.len() <= max && part.bytes().all(allowed);
    if base.is_empty() || name.ends_with('.') || !valid(base, 8) || !valid(ext, 3) {
        return None;
    }
    let mut out = [b' '; 11];
    out.iter_mut()
        .zip(base.bytes())
        .for_each(|(slot, byte)| *slot = byte);
    out.iter_mut()
        .skip(8)
        .zip(ext.bytes())
        .for_each(|(slot, byte)| *slot = byte);
    Some(out)
}

/// 为长文件名生成 `BASIS~N.EXT` 短名。
fn short_alias(name: &str, seq: usize) -> [u8; 11] {
    let (base, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let clean = |part: &str| -> Vec<u8> {
        part.bytes()
            .filter(u8::is_ascii_alphanumeric)
            .map(|byte| byte.to_ascii_uppercase())
            .collect()
    };
    let suffix = format!("~{seq}");
    let keep = 8_usize.saturating_sub(suffix.len());
    let mut out = [b' '; 11];
    let base_bytes: Vec<u8> = clean(base)
        .into_iter()
        .take(keep)
        .chain(suffix.bytes())
        .collect();
    out.iter_mut()
        .zip(base_bytes)
        .for_each(|(slot, byte)| *slot = byte);
    out.iter_mut()
        .skip(8)
        .zip(clean(ext).into_iter().take(3))
        .for_each(|(slot, byte)| *slot = byte);
    out
}

/// 短名校验和（VFAT）。
fn checksum(short: &[u8; 11]) -> u8 {
    short
        .iter()
        .fold(0_u8, |sum, byte| sum.rotate_right(1).wrapping_add(*byte))
}

/// 一个待写入目录的子项。
#[derive(Debug, Clone, Copy)]
struct Child {
    /// 属性。
    attr: u8,
    /// 起始簇。
    cluster: u16,
    /// 文件大小（目录为 0）。
    size: u32,
}

/// 镜像写入状态。
#[derive(Debug)]
struct Writer {
    /// 镜像字节。
    image: Vec<u8>,
    /// 布局。
    layout: Layout,
    /// 下一个空闲簇。
    next_cluster: usize,
    /// FAT 表（逻辑值，最后统一编码）。
    fat: Vec<u32>,
    /// 时间戳。
    stamp: Timestamp,
}

impl Writer {
    /// 分配一段连续簇并写好链，返回起始簇（长度为 0 时返回 0）。
    ///
    /// # Errors
    ///
    /// 簇不足时返回 [`ImageError::TooLarge`]。
    fn allocate(&mut self, len: usize) -> Result<usize, ImageError> {
        let count = div_ceil(len, self.layout.cluster_bytes())?;
        if count == 0 {
            return Ok(0);
        }
        let first = self.next_cluster;
        let end = add(first, count)?;
        if end > self.fat.len() {
            return Err(ImageError::TooLarge);
        }
        for cluster in first..end {
            let next = add(cluster, 1)?;
            let value = if next == end {
                0x0FFF_FFFF
            } else {
                to_u32(next)?
            };
            *self.fat.get_mut(cluster).ok_or(ImageError::TooLarge)? = value;
        }
        self.next_cluster = end;
        Ok(first)
    }

    /// 簇号对应的镜像字节偏移。
    ///
    /// # Errors
    ///
    /// 溢出时返回 [`ImageError::TooLarge`]。
    fn cluster_offset(&self, cluster: usize) -> Result<usize, ImageError> {
        let index = cluster.checked_sub(2).ok_or(ImageError::TooLarge)?;
        add(
            self.layout.data_offset()?,
            mul(index, self.layout.cluster_bytes())?,
        )
    }

    /// FAT 日期与时间。
    fn dos_time(&self) -> (u16, u16) {
        let stamp = self.stamp;
        let date = (stamp.year.saturating_sub(1980) << 9_u32)
            | (u16::from(stamp.month) << 5_u32)
            | u16::from(stamp.day);
        let time = (u16::from(stamp.hour) << 11_u32)
            | (u16::from(stamp.minute) << 5_u32)
            | u16::from(stamp.second >> 1_u32);
        (date, time)
    }

    /// 生成一个短名目录条目。
    fn short_entry(&self, short: &[u8; 11], child: Child) -> [u8; ENTRY] {
        let (date, time) = self.dos_time();
        let mut entry = [0_u8; ENTRY];
        let fields: [(usize, &[u8]); 9] = [
            (0, short),
            (11, &[child.attr]),
            (14, &time.to_le_bytes()),
            (16, &date.to_le_bytes()),
            (18, &date.to_le_bytes()),
            (22, &time.to_le_bytes()),
            (24, &date.to_le_bytes()),
            (26, &child.cluster.to_le_bytes()),
            (28, &child.size.to_le_bytes()),
        ];
        for (offset, bytes) in fields {
            if put(&mut entry, offset, bytes).is_err() {
                return entry;
            }
        }
        entry
    }

    /// 写目录内全部条目，返回写入的条目字节。
    ///
    /// # Errors
    ///
    /// 子项写入失败时返回错误。
    fn entries(
        &mut self,
        dir: &Dir,
        self_cluster: Option<(u16, u16)>,
    ) -> Result<Vec<u8>, ImageError> {
        let mut out = Vec::new();
        if let Some((own, parent)) = self_cluster {
            let dot = self.short_entry(
                b".          ",
                Child {
                    attr: ATTR_DIRECTORY,
                    cluster: own,
                    size: 0,
                },
            );
            let dotdot = self.short_entry(
                b"..         ",
                Child {
                    attr: ATTR_DIRECTORY,
                    cluster: parent,
                    size: 0,
                },
            );
            out.extend_from_slice(&dot);
            out.extend_from_slice(&dotdot);
        }
        let own_cluster = self_cluster.map_or(0, |(own, _)| own);
        let mut seq = 0_usize;
        for (name, sub) in &dir.dirs {
            let cluster = self.allocate(dir_bytes(sub)?)?;
            let cluster16 = to_u16(cluster)?;
            let body = self.entries(sub, Some((cluster16, own_cluster)))?;
            let offset = self.cluster_offset(cluster)?;
            put(&mut self.image, offset, &body)?;
            seq = add(seq, 1)?;
            self.named(
                &mut out,
                name,
                seq,
                Child {
                    attr: ATTR_DIRECTORY,
                    cluster: cluster16,
                    size: 0,
                },
            );
        }
        for (name, data) in &dir.files {
            let cluster = self.allocate(data.len())?;
            if cluster != 0 {
                let offset = self.cluster_offset(cluster)?;
                put(&mut self.image, offset, data)?;
            }
            seq = add(seq, 1)?;
            let child = Child {
                attr: ATTR_ARCHIVE,
                cluster: to_u16(cluster)?,
                size: to_u32(data.len())?,
            };
            self.named(&mut out, name, seq, child);
        }
        Ok(out)
    }

    /// 写一个带名字的条目（必要时先写长文件名条目）。
    fn named(&self, out: &mut Vec<u8>, name: &str, seq: usize, child: Child) {
        if let Some(short) = short_name_exact(name) {
            out.extend_from_slice(&self.short_entry(&short, child));
            return;
        }
        let short = short_alias(name, seq);
        let sum = checksum(&short);
        let mut units: Vec<u16> = name.encode_utf16().collect();
        if !units.len().is_multiple_of(13) {
            units.push(0);
        }
        while !units.len().is_multiple_of(13) {
            units.push(0xFFFF);
        }
        let chunks: Vec<&[u16]> = units.chunks(13).collect();
        let count = chunks.len();
        for (index, chunk) in chunks.iter().enumerate().rev() {
            let ordinal = index.saturating_add(1);
            let mut entry = [0_u8; ENTRY];
            let mut ord = u8::try_from(ordinal).unwrap_or(0x3F);
            if ordinal == count {
                ord |= 0x40;
            }
            entry.iter_mut().take(1).for_each(|slot| *slot = ord);
            let offsets = [1_usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
            for (offset, unit) in offsets.iter().zip(chunk.iter()) {
                let bytes = unit.to_le_bytes();
                entry
                    .iter_mut()
                    .skip(*offset)
                    .zip(bytes)
                    .for_each(|(slot, byte)| *slot = byte);
            }
            entry
                .iter_mut()
                .skip(11)
                .take(1)
                .for_each(|slot| *slot = ATTR_LFN);
            entry
                .iter_mut()
                .skip(13)
                .take(1)
                .for_each(|slot| *slot = sum);
            out.extend_from_slice(&entry);
        }
        out.extend_from_slice(&self.short_entry(&short, child));
    }

    /// 写根目录（含卷标条目）。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    fn write_root(&mut self, root: &Dir, label: &str) -> Result<(), ImageError> {
        let volume = self.short_entry(
            &label_bytes(label),
            Child {
                attr: ATTR_VOLUME,
                cluster: 0,
                size: 0,
            },
        );
        let mut body = volume.to_vec();
        body.extend(self.entries(root, None)?);
        put(&mut self.image, self.layout.root_offset()?, &body)
    }

    /// 编码两份 FAT，返回最终镜像。
    ///
    /// # Errors
    ///
    /// 写入失败时返回错误。
    fn finish_fat(mut self) -> Result<Image, ImageError> {
        let mut table = vec![0_u8; mul(self.layout.fat_sectors, SECTOR)?];
        let mut values = self.fat.clone();
        if let Some(first) = values.get_mut(0) {
            *first = 0x0FFF_FFF8;
        }
        if let Some(second) = values.get_mut(1) {
            *second = 0x0FFF_FFFF;
        }
        for (index, value) in values.iter().enumerate() {
            match self.layout.variant {
                Variant::Fat12 => set_fat12(&mut table, index, *value)?,
                Variant::Fat16 => {
                    let short = u16::try_from(*value & 0xFFFF)?;
                    put(&mut table, mul(index, 2)?, &short.to_le_bytes())?;
                }
            }
        }
        let first_fat = SECTOR;
        put(&mut self.image, first_fat, &table)?;
        put(&mut self.image, add(first_fat, table.len())?, &table)?;
        Ok(Image {
            bytes: self.image,
            variant: self.layout.variant,
        })
    }
}

/// 写一个 FAT12 表项。
///
/// # Errors
///
/// 越界时返回 [`ImageError::TooLarge`]。
fn set_fat12(table: &mut [u8], index: usize, value: u32) -> Result<(), ImageError> {
    let entry = u16::try_from(value & 0x0FFF)?;
    let offset = add(index, index.checked_div(2).ok_or(ImageError::TooLarge)?)?;
    let [low, high] = entry.to_le_bytes();
    let next = add(offset, 1)?;
    let current = table.get(offset).copied().ok_or(ImageError::TooLarge)?;
    let following = table.get(next).copied().ok_or(ImageError::TooLarge)?;
    let pair = if index.is_multiple_of(2) {
        [low, (following & 0xF0) | (high & 0x0F)]
    } else {
        [
            (current & 0x0F) | (low << 4_u32),
            (high << 4_u32) | (low >> 4_u32),
        ]
    };
    put(table, offset, &pair)
}

#[cfg(test)]
mod tests;
