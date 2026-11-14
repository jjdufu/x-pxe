//! DHCP 选项 93（客户端系统架构）到 [`ClientArch`] 的映射。

/// 客户端固件架构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientArch {
    /// BIOS / UNDI，选项 93 = 0。
    BiosX86,
    /// UEFI x64（选项 93 = 7 或 9）。
    UefiX64,
    /// UEFI x64 HTTP Boot（选项 93 = 16）。
    UefiX64Http,
    /// 其他架构码，例如 ARM64（11）或未携带选项 93（用 0 以外的原值或缺省）。
    Unsupported(u16),
}

impl ClientArch {
    /// 从选项 93 的 16 位值映射。缺省（`None`）视为不支持。
    #[must_use]
    pub const fn from_option93(code: Option<u16>) -> Self {
        match code {
            Some(0) => Self::BiosX86,
            Some(7 | 9) => Self::UefiX64,
            Some(16) => Self::UefiX64Http,
            Some(other) => Self::Unsupported(other),
            None => Self::Unsupported(u16::MAX),
        }
    }

    /// 是否为已知的 x86 引导架构（含 HTTP Boot）。
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::BiosX86 | Self::UefiX64 | Self::UefiX64Http)
    }
}
