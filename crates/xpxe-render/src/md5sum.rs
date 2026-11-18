//! MD5 摘要（仅用于 d-i `preseed/url/checksum`，不作保密用途）。

/// 计算 16 字节 MD5。
#[must_use]
pub(crate) fn digest(input: &[u8]) -> [u8; 16] {
    let mut state = State::new();
    state.update(input);
    state.finish()
}

/// 小写十六进制 MD5。
#[must_use]
pub(crate) fn hex_digest(input: &[u8]) -> String {
    hex::encode(digest(input))
}

/// MD5 状态。
struct State {
    /// 四个寄存器。
    regs: [u32; 4],
    /// 已处理的总字节数。
    total: u64,
    /// 未满 64 字节的缓冲。
    buffer: [u8; 64],
    /// 缓冲已用长度。
    filled: usize,
}

impl State {
    /// 初始寄存器。
    const fn new() -> Self {
        Self {
            regs: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            total: 0,
            buffer: [0; 64],
            filled: 0,
        }
    }

    /// 追加数据。
    fn update(&mut self, mut input: &[u8]) {
        let added = u64::try_from(input.len()).unwrap_or(u64::MAX);
        self.total = self.total.saturating_add(added);
        if self.filled != 0 {
            let space = 64_usize.saturating_sub(self.filled);
            let take = space.min(input.len());
            if let Some(dst) = self
                .buffer
                .get_mut(self.filled..self.filled.saturating_add(take))
                && let Some(src) = input.get(..take)
            {
                dst.copy_from_slice(src);
                self.filled = self.filled.saturating_add(take);
                input = input.get(take..).unwrap_or(&[]);
            }
            if self.filled == 64 {
                self.process_buffer();
                self.filled = 0;
            }
        }
        while input.len() >= 64 {
            if let Some(block) = input.get(..64) {
                self.process_slice(block);
            }
            input = input.get(64..).unwrap_or(&[]);
        }
        if !input.is_empty()
            && let Some(dst) = self.buffer.get_mut(..input.len())
        {
            dst.copy_from_slice(input);
            self.filled = input.len();
        }
    }

    /// 写入填充并输出。
    fn finish(mut self) -> [u8; 16] {
        let bit_len = self.total.saturating_mul(8);
        let mut pad = [0_u8; 72];
        if let Some(first) = pad.get_mut(0) {
            *first = 0x80;
        }
        let occupied = self.filled.saturating_add(1);
        let pad_zero = if occupied > 56 {
            64_usize.saturating_sub(occupied).saturating_add(56)
        } else {
            56_usize.saturating_sub(occupied)
        };
        let len_at = pad_zero.saturating_add(1);
        write_le_u64(&mut pad, len_at, bit_len);
        let end = len_at.saturating_add(8);
        self.update(pad.get(..end).unwrap_or(&[]));
        regs_to_bytes(self.regs)
    }

    /// 处理内部缓冲。
    fn process_buffer(&mut self) {
        let copy = self.buffer;
        self.process_slice(&copy);
    }

    /// 处理 64 字节块。
    fn process_slice(&mut self, block: &[u8]) {
        let words = block_words(block);
        let [mut aaa, mut bee, mut cee, mut dee] = self.regs;
        for index in 0..64_usize {
            let (fun, word_index, shift, sine) = round_params(index);
            let mixed = match fun {
                RoundFun::FunF => (bee & cee) | (!bee & dee),
                RoundFun::FunG => (bee & dee) | (cee & !dee),
                RoundFun::FunH => bee ^ cee ^ dee,
                RoundFun::FunI => cee ^ (bee | !dee),
            };
            let word = words.get(word_index).copied().unwrap_or(0);
            let sum = aaa
                .wrapping_add(mixed)
                .wrapping_add(word)
                .wrapping_add(sine);
            aaa = dee;
            dee = cee;
            cee = bee;
            bee = bee.wrapping_add(sum.rotate_left(shift));
        }
        add_regs(&mut self.regs, [aaa, bee, cee, dee]);
    }
}

/// 轮函数种类。
enum RoundFun {
    /// F。
    FunF,
    /// G。
    FunG,
    /// H。
    FunH,
    /// I。
    FunI,
}

/// 把块拆成 16 个小端 `u32`。
fn block_words(block: &[u8]) -> [u32; 16] {
    let mut words = [0_u32; 16];
    for (index, slot) in words.iter_mut().enumerate() {
        let start = index.saturating_mul(4);
        if let Some(slice) = block.get(start..start.saturating_add(4))
            && let Ok(bytes) = <[u8; 4]>::try_from(slice)
        {
            *slot = u32::from_le_bytes(bytes);
        }
    }
    words
}

/// 累加寄存器。
fn add_regs(regs: &mut [u32; 4], delta: [u32; 4]) {
    for (slot, extra) in regs.iter_mut().zip(delta) {
        *slot = slot.wrapping_add(extra);
    }
}

/// 输出 16 字节摘要。
fn regs_to_bytes(regs: [u32; 4]) -> [u8; 16] {
    let mut out = [0_u8; 16];
    for (index, value) in regs.iter().enumerate() {
        let bytes = value.to_le_bytes();
        let start = index.saturating_mul(4);
        if let Some(dst) = out.get_mut(start..start.saturating_add(4)) {
            dst.copy_from_slice(&bytes);
        }
    }
    out
}

/// 在 `offset` 处写入小端 `u64`。
fn write_le_u64(buf: &mut [u8], offset: usize, value: u64) {
    let bytes = value.to_le_bytes();
    if let Some(dst) = buf.get_mut(offset..offset.saturating_add(8)) {
        dst.copy_from_slice(&bytes);
    }
}

/// 第 `index` 步的函数、字下标、位移与正弦常数。
fn round_params(index: usize) -> (RoundFun, usize, u32, u32) {
    let sine = *SINE.get(index).unwrap_or(&0);
    if index < 16 {
        (RoundFun::FunF, index, shift_of(&SHIFT_F, index), sine)
    } else if index < 32 {
        let word = rem_16(index.saturating_mul(5).saturating_add(1));
        (
            RoundFun::FunG,
            word,
            shift_of(&SHIFT_G, index.saturating_sub(16)),
            sine,
        )
    } else if index < 48 {
        let word = rem_16(index.saturating_mul(3).saturating_add(5));
        (
            RoundFun::FunH,
            word,
            shift_of(&SHIFT_H, index.saturating_sub(32)),
            sine,
        )
    } else {
        let word = rem_16(index.saturating_mul(7));
        (
            RoundFun::FunI,
            word,
            shift_of(&SHIFT_I, index.saturating_sub(48)),
            sine,
        )
    }
}

/// `value rem 16`。
fn rem_16(value: usize) -> usize {
    value.checked_rem(16).unwrap_or(0)
}

/// 位移表中 `index rem 4` 的项。
fn shift_of(table: &[u32; 4], index: usize) -> u32 {
    let slot = index.checked_rem(4).unwrap_or(0);
    *table.get(slot).unwrap_or(&0)
}

/// F 轮位移。
const SHIFT_F: [u32; 4] = [7, 12, 17, 22];
/// G 轮位移。
const SHIFT_G: [u32; 4] = [5, 9, 14, 20];
/// H 轮位移。
const SHIFT_H: [u32; 4] = [4, 11, 16, 23];
/// I 轮位移。
const SHIFT_I: [u32; 4] = [6, 10, 15, 21];

/// `floor(2^32 × |sin(i+1)|)`，i 从 0 计。
const SINE: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

#[cfg(test)]
mod tests;
