//! 分区方案。

use serde::{Deserialize, Serialize};

use super::host::UnixAbsPath;
use crate::versioning::{FieldError, field_error};

/// 挂载点：交换分区或绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MountPoint {
    /// 交换分区。
    Swap,
    /// 文件系统挂载点。
    Path {
        /// 绝对路径。
        path: UnixAbsPath,
    },
}

impl MountPoint {
    /// 是否为根文件系统。
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.path_is("/")
    }

    /// 是否为 `/boot`。
    #[must_use]
    pub fn is_boot(&self) -> bool {
        self.path_is("/boot")
    }

    /// 是否为交换分区。
    #[must_use]
    pub const fn is_swap(&self) -> bool {
        matches!(self, Self::Swap)
    }

    /// 路径是否等于给定值。
    fn path_is(&self, expected: &str) -> bool {
        match self {
            Self::Path { path } => path.as_str() == expected,
            Self::Swap => false,
        }
    }
}

/// 分区大小。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SizeSpec {
    /// 固定大小（MiB）。
    Mib {
        /// 容量。
        mib: u32,
    },
    /// 使用剩余空间；全表至多一个。
    Rest,
}

impl SizeSpec {
    /// 固定大小是否为正。
    #[must_use]
    pub const fn is_valid(self) -> bool {
        match self {
            Self::Mib { mib } => mib > 0,
            Self::Rest => true,
        }
    }
}

/// 文件系统类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsType {
    /// ext4。
    Ext4,
    /// XFS。
    Xfs,
    /// 交换分区。
    Swap,
    /// FAT32（ESP 等）。
    Vfat,
}

impl FsType {
    /// preseed / partman 名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Xfs => "xfs",
            Self::Swap => "linux-swap",
            Self::Vfat => "fat32",
        }
    }
}

/// 一条自定义分区。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionSpec {
    /// 挂载点。
    pub mount: MountPoint,
    /// 大小。
    pub size: SizeSpec,
    /// 文件系统。
    pub fs: FsType,
    /// 是否位于 LVM 逻辑卷上。
    pub on_lvm: bool,
}

/// 分区方案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartitionScheme {
    /// 现网脚本的 `lvm` + `atomic` + `guided_size max`。
    AtomicLvm,
    /// 非 LVM 的 atomic 配方。
    AtomicPlain,
    /// 自定义配方；UEFI 自动加 512 MiB ESP，BIOS 自动加 1 MiB `biosgrub`。
    Custom {
        /// 分区表（不含自动加入的引导分区）。
        partitions: Vec<PartitionSpec>,
    },
}

impl PartitionScheme {
    /// 收集分区约束错误。
    pub fn collect_errors(&self, errors: &mut Vec<FieldError>) {
        match self {
            Self::AtomicLvm | Self::AtomicPlain => {}
            Self::Custom { partitions } => collect_custom_errors(partitions, errors),
        }
    }
}

/// 自定义分区约束：恰好一个 `/`，至多一个 Rest，至多一个 swap，`/boot` 不在 LVM 上。
fn collect_custom_errors(partitions: &[PartitionSpec], errors: &mut Vec<FieldError>) {
    let mut roots = 0_usize;
    let mut rests = 0_usize;
    let mut swaps = 0_usize;
    for (index, part) in partitions.iter().enumerate() {
        check_one(index, part, errors);
        if part.mount.is_root() {
            roots = roots.saturating_add(1);
        }
        if matches!(part.size, SizeSpec::Rest) {
            rests = rests.saturating_add(1);
        }
        if part.mount.is_swap() {
            swaps = swaps.saturating_add(1);
        }
        if part.mount.is_boot() && part.on_lvm {
            errors.push(field_error(
                &format!("/partitioning/partitions/{index}/on_lvm"),
                "boot_not_on_lvm",
                "/boot 不能放在 LVM 上",
            ));
        }
    }
    if roots != 1 {
        errors.push(field_error(
            "/partitioning/partitions",
            "root_required",
            "自定义分区必须恰好包含一个 /",
        ));
    }
    if rests > 1 {
        errors.push(field_error(
            "/partitioning/partitions",
            "multiple_rest",
            "至多一个分区可以使用剩余空间",
        ));
    }
    if swaps > 1 {
        errors.push(field_error(
            "/partitioning/partitions",
            "multiple_swap",
            "至多一个 swap 分区",
        ));
    }
}

/// 单条分区的文件系统与大小约束。
fn check_one(index: usize, part: &PartitionSpec, errors: &mut Vec<FieldError>) {
    if !part.size.is_valid() {
        errors.push(field_error(
            &format!("/partitioning/partitions/{index}/size"),
            "zero_size",
            "固定大小必须大于 0",
        ));
    }
    let swap_fs = matches!(part.fs, FsType::Swap);
    if part.mount.is_swap() != swap_fs {
        errors.push(field_error(
            &format!("/partitioning/partitions/{index}/fs"),
            "fs_mismatch",
            "swap 挂载点必须使用 linux-swap，其它挂载点不能使用 linux-swap",
        ));
    }
    if part.mount.is_root() && swap_fs {
        errors.push(field_error(
            &format!("/partitioning/partitions/{index}/fs"),
            "root_not_swap",
            "/ 不能使用 linux-swap",
        ));
    }
}
