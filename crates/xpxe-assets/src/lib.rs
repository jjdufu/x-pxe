//! x-pxe 资产：`OpenPGP` 验证子集、Debian 签名链、iPXE 格式检查、内核 ABI 与镜像对齐、
//! 受控下载与资产频道（激活、金丝雀、回滚）。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

mod align;
mod armor;
mod catalog;
mod error;
mod fetch;
mod ingest;
mod ipxe;
mod kernel;
mod kind;
mod packet;
mod pubkey;
mod release;
mod signature;
mod trust;

pub use align::{
    Alignment, Compression, PACKAGES_MAX, check_alignment, decompress_packages, kernel_abis,
};
pub use catalog::{Actor, CHANNELS_OBJECT, CanaryRun, Catalog, Operation, VerifiedAsset};
pub use error::{AssetsError, AssetsResult};
pub use fetch::{
    FetchMeta, FetchPolicy, Fetched, Fetcher, FileTarget, MAX_REDIRECTS, OFFICIAL_HOSTS, Origin,
    StoredFile, store_bytes,
};
pub use ingest::{
    AlignmentReport, INSTALLER_SUMS, Ingest, Mirror, NETBOOT_INITRD, NETBOOT_LINUX, NetbootExpect,
    NetbootFiles, PACKAGES_GZ, PACKAGES_XZ, Source, VerifiedRelease, check_release_entry,
    finish_netboot, netboot_expectations, verify_ipxe, verify_release,
};
pub use ipxe::{IpxeFormat, check_efi_x64, check_undionly};
pub use kernel::{KernelVersion, kernel_version};
pub use kind::{ALL_KINDS, AssetChannels, AssetKind, ChannelEntry, parse_kind};
pub use pubkey::{PublicKeyInfo, parse_public_key};
pub use release::{Release, ReleaseEntry, STALE_AFTER, Sha256Sums, is_sha256_hex};
pub use trust::{ClearSigned, Signer, TrustStore};
