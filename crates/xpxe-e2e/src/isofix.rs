//! 手动引导用例的 ISO 夹具：生成带子目录与 Rock Ridge 长名的最小 ISO9660。
//!
//! 布局：主卷描述符 → 终止符 → 根目录 → 每个子目录一个扇区 → 文件内容（每个文件占整扇区）。
//! 只用于测试；文件与目录都必须能放进单个 2048 字节目录扇区。

use crate::Outcome;

/// 扇区大小。
const SECTOR: usize = 2048;
/// 根目录扇区。
const ROOT_LBA: u32 = 18;

/// 夹具中的一个文件：路径为 `/<目录>/<名字>` 或 `/<名字>`（最多一级子目录）。
#[derive(Debug, Clone, Copy)]
pub struct FixtureFile<'data> {
    /// 绝对路径。
    pub path: &'data str,
    /// 内容。
    pub data: &'data [u8],
}

/// 在 `offset` 处写入。
///
/// # Errors
///
/// 越界时返回错误。
fn put(img: &mut [u8], offset: usize, bytes: &[u8]) -> Outcome {
    let end = offset.checked_add(bytes.len()).ok_or("溢出")?;
    img.get_mut(offset..end)
        .ok_or("越界")?
        .copy_from_slice(bytes);
    Ok(())
}

/// 扇区起点。
///
/// # Errors
///
/// 溢出时返回错误。
fn at(lba: u32) -> Outcome<usize> {
    usize::try_from(lba)?
        .checked_mul(SECTOR)
        .ok_or_else(|| "溢出".into())
}

/// ISO9660 一级名（大写，文件追加 `;1`）。
fn plain(name: &str, dir: bool) -> Vec<u8> {
    let upper: String = name
        .chars()
        .map(|item| {
            if item.is_ascii_alphanumeric() {
                item.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if dir {
        upper.into_bytes()
    } else {
        format!("{upper};1").into_bytes()
    }
}

/// 目录记录（`nm` 为 Rock Ridge 名，空表示 `.`/`..`）。
///
/// # Errors
///
/// 长度超限时返回错误。
fn record(name: &[u8], extent: (u32, u32), dir: bool, nm: &str) -> Outcome<Vec<u8>> {
    let (lba, len) = extent;
    let mut rec = vec![0_u8; 33];
    rec.extend_from_slice(name);
    if name.len().is_multiple_of(2) {
        rec.push(0);
    }
    if !nm.is_empty() {
        rec.extend_from_slice(&[b'N', b'M', u8::try_from(nm.len().saturating_add(5))?, 1, 0]);
        rec.extend_from_slice(nm.as_bytes());
    }
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

/// 拆分路径为（目录，名字）。
///
/// # Errors
///
/// 不是 `/a` 或 `/d/a` 形式时返回错误。
fn split(path: &str) -> Outcome<(&str, &str)> {
    let rest = path.strip_prefix('/').ok_or("路径须以 / 开头")?;
    match rest.split_once('/') {
        Some((dir, name)) if !name.contains('/') => Ok((dir, name)),
        Some(_) => Err("最多一级子目录".into()),
        None => Ok(("", rest)),
    }
}

/// 扇区数（向上取整，至少 1）。
///
/// # Errors
///
/// 溢出时返回错误。
fn sectors(len: usize) -> Outcome<u32> {
    Ok(u32::try_from(len.div_ceil(SECTOR).max(1))?)
}

/// 生成 ISO。
///
/// # Errors
///
/// 路径不合法或目录放不进一个扇区时返回错误。
pub fn build(volume_id: &str, files: &[FixtureFile<'_>]) -> Outcome<Vec<u8>> {
    let mut dirs: Vec<&str> = Vec::new();
    for file in files {
        let (dir, _name) = split(file.path)?;
        if !dir.is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    let dir_lba = |index: usize| -> Outcome<u32> {
        Ok(ROOT_LBA
            .saturating_add(1)
            .saturating_add(u32::try_from(index)?))
    };
    let mut next = dir_lba(dirs.len())?;
    let mut placed = Vec::new();
    for file in files {
        placed.push((*file, next));
        next = next.saturating_add(sectors(file.data.len())?);
    }
    let mut img = vec![0_u8; at(next)?];
    write_pvd(&mut img, volume_id)?;
    let mut root = root_dot(&mut img)?;
    root.extend(record(&[1], (ROOT_LBA, 2048), true, "")?);
    for (index, dir) in dirs.iter().enumerate() {
        root.extend(record(
            &plain(dir, true),
            (dir_lba(index)?, 2048),
            true,
            dir,
        )?);
    }
    let mut subs: Vec<Vec<u8>> = Vec::new();
    for index in 0..dirs.len() {
        let mut sub = record(&[0], (dir_lba(index)?, 2048), true, "")?;
        sub.extend(record(&[1], (ROOT_LBA, 2048), true, "")?);
        subs.push(sub);
    }
    for (file, lba) in &placed {
        let (dir, name) = split(file.path)?;
        let rec = record(
            &plain(name, false),
            (*lba, u32::try_from(file.data.len())?),
            false,
            name,
        )?;
        match dirs.iter().position(|item| *item == dir) {
            Some(index) => subs.get_mut(index).ok_or("目录")?.extend(rec),
            None => root.extend(rec),
        }
        put(&mut img, at(*lba)?, file.data)?;
    }
    if root.len() > SECTOR || subs.iter().any(|sub| sub.len() > SECTOR) {
        return Err("目录超过一个扇区".into());
    }
    put(&mut img, at(ROOT_LBA)?, &root)?;
    for (index, sub) in subs.iter().enumerate() {
        put(&mut img, at(dir_lba(index)?)?, sub)?;
    }
    Ok(img)
}

/// 根目录 `.` 记录（带 SUSP `SP`，表明使用 Rock Ridge），并写入主卷描述符的根记录。
///
/// # Errors
///
/// 越界时返回错误。
fn root_dot(img: &mut [u8]) -> Outcome<Vec<u8>> {
    let mut root = record(&[0], (ROOT_LBA, 2048), true, "")?;
    drop(root.splice(34..34, [b'S', b'P', 7, 1, 0xBE, 0xEF, 0]));
    let fix = u8::try_from(root.len())?;
    put(&mut root, 0, &[fix])?;
    put(
        img,
        at(16)?.saturating_add(156),
        root.get(..34).ok_or("短")?,
    )?;
    Ok(root)
}

/// 写主卷描述符与终止符。
///
/// # Errors
///
/// 越界时返回错误。
fn write_pvd(img: &mut [u8], volume_id: &str) -> Outcome {
    let pvd = at(16)?;
    put(img, pvd, b"\x01CD001")?;
    let mut label = volume_id.as_bytes().to_vec();
    label.resize(32, b' ');
    put(img, pvd.saturating_add(40), label.get(..32).ok_or("卷标")?)?;
    put(img, at(17)?, b"\xFFCD001")?;
    Ok(())
}
