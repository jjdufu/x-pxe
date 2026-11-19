//! 流式 ISO9660 读取：目录遍历（Rock Ridge 长名优先，回落 Joliet 与 ISO9660）、按路径查找、流式拷贝文件，
//! 以及 hybrid（MBR/GPT）与已知发行版特征识别。
//!
//! 只按需读取扇区，不把整份 ISO 读入内存；目录深度、条目数、目录大小与续接区数量都有上限，并检测目录环。

use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::ImageError;
use crate::cpio;

/// 逻辑扇区大小。
const SECTOR: u64 = 2048;
/// 目录嵌套上限。
const MAX_DEPTH: usize = 16;
/// 遍历条目总数上限。
const MAX_ENTRIES: usize = 200_000;
/// 单个目录的字节数上限。
const MAX_DIR_BYTES: u32 = 16 * 1024 * 1024;
/// 单条记录跟随的 SUSP 续接区（CE）上限。
const MAX_CE: usize = 8;
/// 卷描述符扫描范围（扇区 16 起）。
const MAX_DESCRIPTORS: u64 = 64;

/// 目录项（文件或目录）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoEntry {
    /// 以 `/` 开头的完整路径。
    pub path: String,
    /// 是否为目录。
    pub is_dir: bool,
    /// 文件大小（多段文件为各段之和）。
    pub size: u64,
    /// 数据段：起始扇区与字节长度。
    pub extents: Vec<(u32, u32)>,
}

/// 卷信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoInfo {
    /// 卷标（去掉尾部空格）。
    pub volume_id: String,
    /// 镜像总字节数。
    pub size: u64,
    /// 引导特征。
    pub boot: BootTraits,
    /// 文件名扩展。
    pub names: NameTraits,
}

/// 引导特征。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootTraits {
    /// 是否 hybrid（扇区 0 含 MBR 分区表或 GPT 头）。
    pub hybrid: bool,
    /// 是否有 El Torito 引导记录。
    pub el_torito: bool,
}

/// 文件名扩展。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameTraits {
    /// 是否有 Joliet 补充卷描述符。
    pub joliet: bool,
    /// 根目录是否带 Rock Ridge（SUSP `SP`）。
    pub rock_ridge: bool,
}

/// 文件名来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Names {
    /// Rock Ridge `NM`（`skip` 为 SUSP 跳过字节数）。
    RockRidge(usize),
    /// Joliet（UCS-2 大端）。
    Joliet,
    /// ISO9660 原名。
    Plain,
}

/// 目录位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirLoc {
    /// 起始扇区。
    lba: u32,
    /// 字节长度。
    len: u32,
}

/// 流式 ISO 读取器。
#[derive(Debug)]
pub struct IsoReader<R> {
    /// 底层读取器。
    inner: R,
    /// 遍历使用的根目录。
    root: DirLoc,
    /// 文件名来源。
    names: Names,
    /// 卷信息。
    info: IsoInfo,
}

/// 解析出的单条目录记录。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    /// 解码后的文件名。
    name: String,
    /// 种类。
    kind: Kind,
    /// 多段文件标志（后面还有同名段）。
    multi: bool,
    /// 起始扇区。
    lba: u32,
    /// 字节长度。
    len: u32,
}

/// 目录记录种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// 普通文件。
    File,
    /// 目录。
    Dir,
    /// Rock Ridge `RE`：被重定位的目录占位，遍历时跳过。
    Relocated,
}

impl<R: Read + Seek> IsoReader<R> {
    /// 打开 ISO：读卷描述符、判定文件名来源与 hybrid。
    ///
    /// # Errors
    ///
    /// 缺少主卷描述符、签名错误或读失败时返回错误。
    pub fn open(mut inner: R) -> Result<Self, ImageError> {
        let size = inner.seek(SeekFrom::End(0))?;
        let mut primary = None;
        let mut joliet_root = None;
        let mut el_torito = false;
        for index in 16..16_u64.saturating_add(MAX_DESCRIPTORS) {
            let desc = read_at(&mut inner, index.saturating_mul(SECTOR), 2048)?;
            if part(&desc, 1, 5)? != b"CD001" {
                return Err(bad("卷描述符签名错误"));
            }
            match byte(&desc, 0)? {
                1 => primary = Some(desc),
                2 if is_joliet(part(&desc, 88, 32)?) => joliet_root = Some(root_loc(&desc)?),
                0 => el_torito |= part(&desc, 7, 23)? == b"EL TORITO SPECIFICATION",
                255 => break,
                _other => {}
            }
        }
        let pvd = primary.ok_or_else(|| bad("缺少主卷描述符"))?;
        let volume_id = String::from_utf8_lossy(part(&pvd, 40, 32)?)
            .trim_end()
            .to_owned();
        let plain_root = root_loc(&pvd)?;
        let hybrid = detect_hybrid(&mut inner, size)?;
        let rr_skip = rock_ridge_skip(&mut inner, plain_root)?;
        let (root, names) = match (rr_skip, joliet_root) {
            (Some(skip), _) => (plain_root, Names::RockRidge(skip)),
            (None, Some(joliet)) => (joliet, Names::Joliet),
            (None, None) => (plain_root, Names::Plain),
        };
        let info = IsoInfo {
            volume_id,
            size,
            boot: BootTraits { hybrid, el_torito },
            names: NameTraits {
                joliet: joliet_root.is_some(),
                rock_ridge: rr_skip.is_some(),
            },
        };
        Ok(Self {
            inner,
            root,
            names,
            info,
        })
    }

    /// 卷信息。
    #[must_use]
    pub const fn info(&self) -> &IsoInfo {
        &self.info
    }

    /// 遍历全部目录项（广度优先，路径排序前的发现顺序）。
    ///
    /// # Errors
    ///
    /// 超过深度/条目上限、发现目录环或读失败时返回错误。
    pub fn walk(&mut self) -> Result<Vec<IsoEntry>, ImageError> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::from([self.root.lba]);
        let mut queue = vec![(String::new(), self.root, 0_usize)];
        while let Some((prefix, dir, depth)) = queue.pop() {
            if depth >= MAX_DEPTH {
                return Err(bad("目录嵌套过深"));
            }
            for entry in self.list(&prefix, dir)? {
                enqueue(&entry, depth, &mut seen, &mut queue)?;
                out.push(entry);
            }
            if out.len() > MAX_ENTRIES {
                return Err(bad("目录项过多"));
            }
        }
        out.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(out)
    }

    /// 按路径查找（逐级比较；先精确匹配，ISO9660 原名再忽略大小写）。
    ///
    /// # Errors
    ///
    /// 读失败或目录结构不合法时返回错误。
    pub fn find(&mut self, path: &str) -> Result<Option<IsoEntry>, ImageError> {
        let mut dir = self.root;
        let mut prefix = String::new();
        let parts: Vec<&str> = path.split('/').filter(|piece| !piece.is_empty()).collect();
        for (index, piece) in parts.iter().enumerate() {
            let entries = self.list(&prefix, dir)?;
            let Some(entry) = match_name(&entries, piece) else {
                return Ok(None);
            };
            if index.saturating_add(1) == parts.len() {
                return Ok(Some(entry));
            }
            let Some(child) = dir_of(&entry) else {
                return Ok(None);
            };
            dir = child;
            prefix = entry.path;
        }
        Ok(None)
    }

    /// 把文件内容流式写入 `out`，返回字节数。
    ///
    /// # Errors
    ///
    /// 目录项是目录、数据段越界或读写失败时返回错误。
    pub fn copy_to<W: Write>(&mut self, entry: &IsoEntry, out: &mut W) -> Result<u64, ImageError> {
        if entry.is_dir {
            return Err(bad("不能拷贝目录"));
        }
        let mut total = 0_u64;
        for &(lba, len) in &entry.extents {
            let start = u64::from(lba).saturating_mul(SECTOR);
            if start.saturating_add(u64::from(len)) > self.info.size {
                return Err(bad("文件数据越界"));
            }
            let _pos = self.inner.seek(SeekFrom::Start(start))?;
            total = total.saturating_add(cpio::copy_exact(&mut self.inner, out, u64::from(len))?);
        }
        Ok(total)
    }

    /// 读取小文件内容（超过 `limit` 字节即拒绝）。
    ///
    /// # Errors
    ///
    /// 超过上限或读失败时返回错误。
    pub fn read_small(&mut self, entry: &IsoEntry, limit: u64) -> Result<Vec<u8>, ImageError> {
        if entry.size > limit {
            return Err(ImageError::TooLarge);
        }
        let mut out = Vec::new();
        let _len = self.copy_to(entry, &mut out)?;
        Ok(out)
    }

    /// 生成“原 initrd + newc 段（整份 ISO，文件名 `iso_name`）+ 结束标记”。返回写出的字节数。
    ///
    /// # Errors
    ///
    /// 文件名非法、ISO 超过 newc 4 GiB 上限或读写失败时返回错误。
    pub fn write_initrd_with_iso<W: Write>(
        &mut self,
        initrd: &IsoEntry,
        iso_name: &str,
        out: &mut W,
    ) -> Result<u64, ImageError> {
        let mut total = self.copy_to(initrd, out)?;
        let pad = cpio::pad4(total);
        out.write_all(&cpio::zeros(pad)?)?;
        total = total.saturating_add(pad);
        let size = self.info.size;
        let _start = self.inner.seek(SeekFrom::Start(0))?;
        let name = iso_name.trim_start_matches('/');
        total = total.saturating_add(cpio::append_file(out, name, size, &mut self.inner, 1)?);
        let tail = cpio::trailer()?;
        out.write_all(&tail)?;
        Ok(total.saturating_add(u64::try_from(tail.len())?))
    }

    /// 列出一个目录（不含 `.` 与 `..`，合并多段文件）。
    ///
    /// # Errors
    ///
    /// 目录过大、记录越界或读失败时返回错误。
    fn list(&mut self, prefix: &str, dir: DirLoc) -> Result<Vec<IsoEntry>, ImageError> {
        if dir.len > MAX_DIR_BYTES {
            return Err(bad("目录过大"));
        }
        let data = read_at(
            &mut self.inner,
            u64::from(dir.lba).saturating_mul(SECTOR),
            usize::try_from(dir.len)?,
        )?;
        let mut out: Vec<IsoEntry> = Vec::new();
        let mut pending_multi = false;
        let mut pos = 0_usize;
        while pos < data.len() {
            let rec_len = usize::from(byte(&data, pos)?);
            if rec_len == 0 {
                pos = next_sector(pos)?;
                continue;
            }
            let raw = part(&data, pos, rec_len)?;
            pos = pos.checked_add(rec_len).ok_or(ImageError::TooLarge)?;
            if let Some(record) = self.record(raw)? {
                let multi = record.multi;
                push_record(&mut out, prefix, &record, pending_multi);
                pending_multi = multi;
            }
        }
        Ok(out)
    }

    /// 解析一条目录记录；`.`、`..` 返回 `None`。
    ///
    /// # Errors
    ///
    /// 记录越界或续接区读失败时返回错误。
    fn record(&mut self, raw: &[u8]) -> Result<Option<Record>, ImageError> {
        let name_len = usize::from(byte(raw, 32)?);
        let id = part(raw, 33, name_len)?;
        if id == [0] || id == [1] {
            return Ok(None);
        }
        let flags = byte(raw, 25)?;
        let mut record = Record {
            name: String::new(),
            kind: if flags & 0x02 == 0 {
                Kind::File
            } else {
                Kind::Dir
            },
            multi: flags & 0x80 != 0,
            lba: le32(raw, 2)?,
            len: le32(raw, 10)?,
        };
        record.name = match self.names {
            Names::RockRidge(skip) => {
                let su_start = su_offset(name_len).saturating_add(skip);
                let su = raw.get(su_start..).unwrap_or_default().to_vec();
                self.rock_ridge(&su, &mut record)?
                    .unwrap_or_else(|| plain_name(id))
            }
            Names::Joliet => joliet_name(id),
            Names::Plain => plain_name(id),
        };
        if record.name.is_empty() || record.name.contains('/') || record.name == ".." {
            return Err(bad("非法文件名"));
        }
        Ok(Some(record))
    }

    /// 解析 SUSP 系统使用区：`NM` 长名、`CL` 子目录链接、`RE` 重定位标记、`CE` 续接区。
    ///
    /// # Errors
    ///
    /// 续接区越界或读失败时返回错误。
    fn rock_ridge(
        &mut self,
        first: &[u8],
        record: &mut Record,
    ) -> Result<Option<String>, ImageError> {
        let mut name: Option<Vec<u8>> = None;
        let mut area = first.to_vec();
        for _round in 0..MAX_CE {
            let Some((block, offset, length)) = parse_susp(&area, record, &mut name)? else {
                break;
            };
            let start = u64::from(block)
                .saturating_mul(SECTOR)
                .saturating_add(u64::from(offset));
            area = read_at(&mut self.inner, start, usize::try_from(length.min(2048))?)?;
        }
        if record.kind == Kind::Dir && record.len == 0 {
            // CL 指向的子目录：从其 `.` 记录取真实长度
            let dot = read_at(
                &mut self.inner,
                u64::from(record.lba).saturating_mul(SECTOR),
                34,
            )?;
            record.len = le32(&dot, 10)?;
        }
        Ok(name.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
    }
}

/// SUSP `CE` 续接区：块、偏移、长度。
type Continuation = (u32, u32, u32);

/// 解析一段 SUSP 项；返回 `CE` 续接区（块、偏移、长度）。
///
/// # Errors
///
/// 项越界时返回错误。
fn parse_susp(
    area: &[u8],
    record: &mut Record,
    name: &mut Option<Vec<u8>>,
) -> Result<Option<Continuation>, ImageError> {
    let mut next = None;
    let mut pos = 0_usize;
    while pos.saturating_add(4) <= area.len() {
        let len = usize::from(byte(area, pos.saturating_add(2))?);
        if len < 4 {
            break;
        }
        let entry = part(area, pos, len)?;
        match part(entry, 0, 2)? {
            b"NM" if byte(entry, 4)? & 0x06 == 0 => name
                .get_or_insert_with(Vec::new)
                .extend_from_slice(entry.get(5..).unwrap_or_default()),
            b"CL" => {
                record.kind = Kind::Dir;
                record.lba = le32(entry, 4)?;
                record.len = 0;
            }
            b"RE" => record.kind = Kind::Relocated,
            b"CE" => next = Some((le32(entry, 4)?, le32(entry, 12)?, le32(entry, 20)?)),
            b"ST" => break,
            _other => {}
        }
        pos = pos.saturating_add(len);
    }
    Ok(next)
}

/// 待遍历目录：路径前缀、位置、深度。
type Pending = (String, DirLoc, usize);

/// 子目录入队；同一扇区再次出现即判为目录环。
///
/// # Errors
///
/// 发现目录环时返回错误。
fn enqueue(
    entry: &IsoEntry,
    depth: usize,
    seen: &mut BTreeSet<u32>,
    queue: &mut Vec<Pending>,
) -> Result<(), ImageError> {
    let Some(loc) = dir_of(entry) else {
        return Ok(());
    };
    if !seen.insert(loc.lba) {
        return Err(bad("目录环"));
    }
    queue.push((entry.path.clone(), loc, depth.saturating_add(1)));
    Ok(())
}

/// 目录项指向的子目录位置。
fn dir_of(entry: &IsoEntry) -> Option<DirLoc> {
    if !entry.is_dir {
        return None;
    }
    entry.extents.first().map(|&(lba, len)| DirLoc { lba, len })
}

/// 同一目录中按名字匹配：先精确，再忽略 ASCII 大小写。
fn match_name(entries: &[IsoEntry], piece: &str) -> Option<IsoEntry> {
    let base = |entry: &&IsoEntry| entry.path.rsplit('/').next().map(str::to_owned);
    entries
        .iter()
        .find(|entry| base(entry).as_deref() == Some(piece))
        .or_else(|| {
            entries
                .iter()
                .find(|entry| base(entry).is_some_and(|name| name.eq_ignore_ascii_case(piece)))
        })
        .cloned()
}

/// 把一条记录并入目录列表：多段文件的后续段追加到上一项。
fn push_record(out: &mut Vec<IsoEntry>, prefix: &str, record: &Record, pending_multi: bool) {
    let suffix = format!("/{}", record.name);
    if record.kind == Kind::Relocated {
        return;
    }
    if let Some(last) = out
        .last_mut()
        .filter(|last| pending_multi && last.path.ends_with(&suffix))
    {
        last.extents.push((record.lba, record.len));
        last.size = last.size.saturating_add(u64::from(record.len));
        return;
    }
    let is_dir = record.kind == Kind::Dir;
    out.push(IsoEntry {
        path: format!("{prefix}{suffix}"),
        is_dir,
        size: if is_dir { 0 } else { u64::from(record.len) },
        extents: vec![(record.lba, record.len)],
    });
}

/// 系统使用区相对记录起点的偏移（名字长度为偶数时有 1 字节填充）。
fn su_offset(name_len: usize) -> usize {
    33_usize
        .saturating_add(name_len)
        .saturating_add(usize::from(name_len.is_multiple_of(2)))
}

/// 识别出的发行版家族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Proxmox 系（`/boot/linux26` + `/boot/initrd.img`）。
    Pve,
    /// Ubuntu 系 `casper`。
    Casper,
    /// Debian live。
    Live,
    /// Debian 安装器（`/install.amd`）。
    DebianInstaller,
    /// 未识别。
    Unknown,
}

impl Family {
    /// 稳定名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pve => "pve",
            Self::Casper => "casper",
            Self::Live => "live",
            Self::DebianInstaller => "debian_installer",
            Self::Unknown => "unknown",
        }
    }
}

/// 识别结果与推荐条目参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// 家族。
    pub family: Family,
    /// 推荐内核路径（未识别时为空）。
    pub kernel_path: Option<String>,
    /// 推荐 initrd 路径。
    pub initrd_path: Option<String>,
    /// 是否推荐把 ISO 打进 initrd。
    pub embed_iso: bool,
    /// 推荐的 ISO 在 initrd 中的文件名。
    pub iso_name: Option<String>,
}

/// 按特征文件识别发行版（特征优先于版本号）。
#[must_use]
pub fn detect(entries: &[IsoEntry]) -> Detection {
    let has = |path: &str| {
        entries
            .iter()
            .any(|entry| !entry.is_dir && entry.path == path)
    };
    if has("/boot/linux26") && has("/boot/initrd.img") {
        return Detection {
            family: Family::Pve,
            kernel_path: Some("/boot/linux26".to_owned()),
            initrd_path: Some("/boot/initrd.img".to_owned()),
            embed_iso: true,
            iso_name: Some("/proxmox.iso".to_owned()),
        };
    }
    [
        (Family::Casper, "/casper/"),
        (Family::Live, "/live/"),
        (Family::DebianInstaller, "/install.amd/"),
    ]
    .into_iter()
    .find_map(|(family, dir)| {
        let kernel_path = first_in(entries, dir, "vmlinuz")?;
        let initrd_path = first_in(entries, dir, "initrd")?;
        Some(Detection {
            family,
            kernel_path: Some(kernel_path),
            initrd_path: Some(initrd_path),
            embed_iso: false,
            iso_name: None,
        })
    })
    .unwrap_or(Detection {
        family: Family::Unknown,
        kernel_path: None,
        initrd_path: None,
        embed_iso: false,
        iso_name: None,
    })
}

/// 目录 `dir` 下（不含子目录）第一个以 `stem` 开头的文件。
fn first_in(entries: &[IsoEntry], dir: &str, stem: &str) -> Option<String> {
    entries
        .iter()
        .filter(|entry| !entry.is_dir)
        .map(|entry| entry.path.as_str())
        .find(|path| {
            path.strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with(stem) && !rest.contains('/'))
        })
        .map(str::to_owned)
}

/// Joliet 转义序列（UCS-2 Level 1–3）。
fn is_joliet(escape: &[u8]) -> bool {
    [b"%/@", b"%/C", b"%/E"]
        .iter()
        .any(|seq| escape.starts_with(seq.as_slice()))
}

/// 卷描述符中的根目录位置。
///
/// # Errors
///
/// 描述符过短时返回错误。
fn root_loc(desc: &[u8]) -> Result<DirLoc, ImageError> {
    let root = part(desc, 156, 34)?;
    Ok(DirLoc {
        lba: le32(root, 2)?,
        len: le32(root, 10)?,
    })
}

/// 扇区 0 是否含 MBR 分区表（55AA + 至少一个非空分区类型）或 GPT 头。
///
/// # Errors
///
/// 读失败时返回错误。
fn detect_hybrid<R: Read + Seek>(inner: &mut R, size: u64) -> Result<bool, ImageError> {
    if size < 1024 {
        return Ok(false);
    }
    let head = read_at(inner, 0, 1024)?;
    let mbr = part(&head, 510, 2)? == [0x55, 0xAA]
        && (0..4_usize).any(|slot| {
            byte(
                &head,
                446_usize
                    .saturating_add(slot.saturating_mul(16))
                    .saturating_add(4),
            )
            .is_ok_and(|kind| kind != 0)
        });
    let gpt = part(&head, 512, 8)? == b"EFI PART";
    Ok(mbr || gpt)
}

/// 根目录 `.` 记录的 SUSP `SP` 项：存在即为 Rock Ridge，返回跳过字节数。
///
/// # Errors
///
/// 读失败时返回错误。
fn rock_ridge_skip<R: Read + Seek>(
    inner: &mut R,
    root: DirLoc,
) -> Result<Option<usize>, ImageError> {
    let data = read_at(inner, u64::from(root.lba).saturating_mul(SECTOR), 256)?;
    let rec_len = usize::from(byte(&data, 0)?);
    let name_len = usize::from(byte(&data, 32)?);
    let su_start = su_offset(name_len);
    let su = data.get(su_start..rec_len).unwrap_or_default();
    if su.len() >= 7 && su.starts_with(b"SP") && part(su, 4, 2)? == [0xBE, 0xEF] {
        return Ok(Some(usize::from(byte(su, 6)?)));
    }
    Ok(None)
}

/// ISO9660 原名：去掉 `;1` 与末尾 `.`。
fn plain_name(id: &[u8]) -> String {
    let text = String::from_utf8_lossy(id);
    let base = text.split(';').next().unwrap_or_default();
    base.strip_suffix('.').unwrap_or(base).to_owned()
}

/// Joliet 名：UCS-2 大端，去掉 `;1`。
fn joliet_name(id: &[u8]) -> String {
    let units: Vec<u16> = id
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .collect();
    let text = String::from_utf16_lossy(&units);
    text.split(';').next().unwrap_or_default().to_owned()
}

/// 下一扇区起点。
///
/// # Errors
///
/// 溢出时返回错误。
fn next_sector(pos: usize) -> Result<usize, ImageError> {
    let sector = usize::try_from(SECTOR)?;
    pos.checked_div(sector)
        .and_then(|index| index.checked_add(1))
        .and_then(|index| index.checked_mul(sector))
        .ok_or(ImageError::TooLarge)
}

/// 在 `offset` 处读 `len` 字节。
///
/// # Errors
///
/// 越过镜像末尾或读失败时返回错误。
fn read_at<R: Read + Seek>(inner: &mut R, offset: u64, len: usize) -> Result<Vec<u8>, ImageError> {
    let _pos = inner.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0_u8; len];
    inner
        .read_exact(&mut buf)
        .map_err(|_error| bad("读取越过镜像末尾"))?;
    Ok(buf)
}

/// 取子切片。
///
/// # Errors
///
/// 越界时返回错误。
fn part(data: &[u8], offset: usize, len: usize) -> Result<&[u8], ImageError> {
    let end = offset.checked_add(len).ok_or(ImageError::TooLarge)?;
    data.get(offset..end).ok_or_else(|| bad("记录越界"))
}

/// 取单字节。
///
/// # Errors
///
/// 越界时返回错误。
fn byte(data: &[u8], offset: usize) -> Result<u8, ImageError> {
    data.get(offset).copied().ok_or_else(|| bad("记录越界"))
}

/// 小端 `u32`。
///
/// # Errors
///
/// 越界时返回错误。
fn le32(data: &[u8], offset: usize) -> Result<u32, ImageError> {
    let raw = part(data, offset, 4)?;
    let mut word = [0_u8; 4];
    word.copy_from_slice(raw);
    Ok(u32::from_le_bytes(word))
}

/// 结构错误。
fn bad(what: &str) -> ImageError {
    ImageError::Malformed(what.to_owned())
}

#[cfg(test)]
mod tests;
