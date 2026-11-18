//! `OpenPGP` 报文分帧（RFC 4880 §4.2）与包体读取。

use crate::error::{AssetsResult, fail};

/// 拆出全部报文。
///
/// # Errors
///
/// 任一报文头或长度不合法时返回错误。
pub(crate) fn packets(data: &[u8]) -> AssetsResult<Vec<PacketView<'_>>> {
    let mut out = Vec::new();
    let mut offset = 0_usize;
    while offset < data.len() {
        let parsed = parse_packet(data, offset)?;
        offset = parsed.next;
        out.push(parsed);
    }
    Ok(out)
}

/// 包体顺序读取器（全部越界检查）。
#[derive(Debug, Clone)]
pub(crate) struct Reader<'data> {
    /// 数据。
    data: &'data [u8],
    /// 当前位置。
    pos: usize,
}

impl<'data> Reader<'data> {
    /// 构造读取器。
    pub(crate) const fn new(data: &'data [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// 读取 `len` 字节。
    ///
    /// # Errors
    ///
    /// 越界时返回错误。
    pub(crate) fn take(&mut self, len: usize) -> AssetsResult<&'data [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| fail("truncated", "包体长度溢出"))?;
        let out = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| fail("truncated", "包体截断"))?;
        self.pos = end;
        Ok(out)
    }

    /// 读取一个字节。
    ///
    /// # Errors
    ///
    /// 越界时返回错误。
    pub(crate) fn byte(&mut self) -> AssetsResult<u8> {
        self.take(1)?
            .first()
            .copied()
            .ok_or_else(|| fail("truncated", "包体截断"))
    }

    /// 读取大端 u16。
    ///
    /// # Errors
    ///
    /// 越界时返回错误。
    pub(crate) fn u16(&mut self) -> AssetsResult<u16> {
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_error| fail("truncated", "包体截断"))?;
        Ok(u16::from_be_bytes(bytes))
    }

    /// 读取大端 u32。
    ///
    /// # Errors
    ///
    /// 越界时返回错误。
    pub(crate) fn u32(&mut self) -> AssetsResult<u32> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_error| fail("truncated", "包体截断"))?;
        Ok(u32::from_be_bytes(bytes))
    }

    /// 读取 MPI（2 字节位数 + 大端数值），返回数值字节。
    ///
    /// # Errors
    ///
    /// 越界时返回错误。
    pub(crate) fn mpi(&mut self) -> AssetsResult<&'data [u8]> {
        let bits = usize::from(self.u16()?);
        let len = bits.div_ceil(8);
        self.take(len)
    }

    /// 是否已读完。
    pub(crate) const fn rest_is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// 当前位置。
    pub(crate) const fn position(&self) -> usize {
        self.pos
    }
}

/// 一个报文的解析结果。
pub(crate) struct PacketView<'data> {
    /// 包类型。
    pub(crate) tag: u8,
    /// 包体。
    pub(crate) body: &'data [u8],
    /// 下一包起始偏移。
    next: usize,
}

/// 解析一个报文头与包体。
///
/// # Errors
///
/// 长度字段截断时返回错误。
pub(crate) fn parse_packet(data: &[u8], offset: usize) -> AssetsResult<PacketView<'_>> {
    let first = *data
        .get(offset)
        .ok_or_else(|| fail("truncated", "报文截断"))?;
    if (first & 0x80) == 0 {
        return Err(fail("bad_packet", "非法报文头"));
    }
    if (first & 0x40) == 0 {
        parse_old_packet(data, offset, first)
    } else {
        parse_new_packet(data, offset, first)
    }
}

/// 旧格式报文。
///
/// # Errors
///
/// 长度截断时返回错误。
fn parse_old_packet(data: &[u8], offset: usize, first: u8) -> AssetsResult<PacketView<'_>> {
    let tag = (first & 0x3c).checked_shr(2).unwrap_or(0);
    let length_type = first & 0x03;
    let start = offset
        .checked_add(1)
        .ok_or_else(|| fail("truncated", "报文偏移溢出"))?;
    let (len, body_at) = old_length(data, start, length_type)?;
    slice_packet(data, tag, body_at, len)
}

/// 新格式报文。
///
/// # Errors
///
/// 长度截断时返回错误。
fn parse_new_packet(data: &[u8], offset: usize, first: u8) -> AssetsResult<PacketView<'_>> {
    let tag = first & 0x3f;
    let start = offset
        .checked_add(1)
        .ok_or_else(|| fail("truncated", "报文偏移溢出"))?;
    let (len, body_at) = new_length(data, start)?;
    slice_packet(data, tag, body_at, len)
}

/// 按长度切出包体。
///
/// # Errors
///
/// 超出缓冲区时返回错误。
fn slice_packet(data: &[u8], tag: u8, body_at: usize, len: usize) -> AssetsResult<PacketView<'_>> {
    let end = body_at
        .checked_add(len)
        .ok_or_else(|| fail("truncated", "包长度溢出"))?;
    let body = data
        .get(body_at..end)
        .ok_or_else(|| fail("truncated", "包体超出缓冲区"))?;
    Ok(PacketView {
        tag,
        body,
        next: end,
    })
}

/// 旧格式长度。
///
/// # Errors
///
/// 截断或不定长时返回错误。
fn old_length(data: &[u8], start: usize, length_type: u8) -> AssetsResult<(usize, usize)> {
    match length_type {
        0 => read_len(data, start, 1),
        1 => read_len(data, start, 2),
        2 => read_len(data, start, 4),
        _ => Err(fail("indeterminate", "不支持不定长报文")),
    }
}

/// 新格式长度。
///
/// # Errors
///
/// 截断或部分长度时返回错误。
fn new_length(data: &[u8], start: usize) -> AssetsResult<(usize, usize)> {
    let first = *data
        .get(start)
        .ok_or_else(|| fail("truncated", "缺少长度字节"))?;
    let next = start
        .checked_add(1)
        .ok_or_else(|| fail("truncated", "长度偏移溢出"))?;
    if first < 192 {
        Ok((usize::from(first), next))
    } else if first < 224 {
        let second = *data
            .get(next)
            .ok_or_else(|| fail("truncated", "缺少第二长度字节"))?;
        let hi = usize::from(first)
            .checked_sub(192)
            .and_then(|value| value.checked_mul(256))
            .and_then(|value| value.checked_add(usize::from(second)))
            .and_then(|value| value.checked_add(192))
            .ok_or_else(|| fail("truncated", "长度计算溢出"))?;
        let body_at = next
            .checked_add(1)
            .ok_or_else(|| fail("truncated", "长度偏移溢出"))?;
        Ok((hi, body_at))
    } else if first == 255 {
        read_len(data, next, 4)
    } else {
        Err(fail("partial_length", "不支持部分长度报文"))
    }
}

/// 以大端读取 `width` 字节长度。
///
/// # Errors
///
/// 截断时返回错误。
fn read_len(data: &[u8], start: usize, width: usize) -> AssetsResult<(usize, usize)> {
    let end = start
        .checked_add(width)
        .ok_or_else(|| fail("truncated", "长度字段溢出"))?;
    let bytes = data
        .get(start..end)
        .ok_or_else(|| fail("truncated", "长度字段截断"))?;
    let mut value = 0_usize;
    for byte in bytes {
        value = value
            .checked_mul(256)
            .and_then(|current| current.checked_add(usize::from(*byte)))
            .ok_or_else(|| fail("truncated", "长度溢出"))?;
    }
    Ok((value, end))
}
