//! x-pxe 的纯领域模型。
//!
//! 本 crate 是系统的规则层：受约束的值类型、节点状态机、引导决策与配置版本。
//! 它不做 I/O、不读时钟（时间由调用方传入 [`Timestamp`]）、不依赖 tokio。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

pub mod bootnet;
pub mod decision;
pub mod init;
pub mod inventory;
pub mod lifecycle;
pub mod manual;
pub mod option93;
pub mod provision;
pub mod value;
pub mod versioning;

pub use bootnet::{
    BootNetwork, Counters, DirectiveSnapshot, ExternalCheck, FirmwareSet, Health, Heartbeat,
    HeartbeatRecord, NetIface, Observation, ProbeReport, ReplyStage, RequestSummary, Responder,
    ResponderClass, Sha256Hex, SnapshotEntry, TransferStage,
};
pub use decision::{
    BootDirective, BootOffer, BootTarget, DhcpAnswer, DhcpRequestView, DhcpRules, DirectiveTable,
    FirmwareName, IgnoreReason, IpxeEntry, UefiFirmware, answer, ipxe_entry,
};
pub use inventory::{
    BmcId, BootChannel, FirmwareMode, NodeNetwork, NodeSpec, NodeSpecParts, OsFamily,
};
pub use lifecycle::{
    AuditKind, Effect, EventEnvelope, EventId, EventSource, FailReason, NodeEvent, NodeEventKind,
    NodeRuntime, NodeState, Outcome, Persisted, SessionId, Timestamp, Transition, TransitionError,
    Ttl, apply,
};
pub use manual::{BootMenu, BootMenuEntry, EntryArch, EntryKind};
pub use option93::ClientArch;
pub use provision::{
    AptComponent, AptRepo, ArmoredKey, ComponentSet, ConsoleSet, ConsoleSpec, CryptHash,
    CryptSecret, DiskMatch, DiskRule, DiskTransport, FsType, HostPort, InstallAccounts,
    InstallerDebug, Keymap, Mirror, MirrorScheme, MountPoint, NtpServer, PartitionScheme,
    PartitionSpec, Prefer, PrimaryUser, Profile, ProfileParts, RootAccount, SecuritySource,
    SizeRange, SizeSpec, SuiteSet, TaskName, TaskSet, UnixAbsPath, UnixGroup, Upgrade,
    default_exclude_models, hash_fingerprint,
};
pub use value::{
    BootToken, Fqdn, Hostname, HttpUrl, Ipv4Cidr, Locale, MacAddr, PackageName, ParseError,
    SafeText, SshPublicKey, TimeZone, UnixUser,
};
pub use versioning::{
    CanonicalJson, Change, ChangeValue, ConfigDocument, ConfigError, ConfigKind, FieldError,
    Secret, Timeouts, ValidationError, canonical_json, content_sha256, content_sha256_hex, diff,
    field_error, migrate_current,
};
