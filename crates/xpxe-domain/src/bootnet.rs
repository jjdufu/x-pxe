//! 引导网络配置，以及 `xpxe-bootnet` 与控制面内部 API 之间的载荷。
//!
//! - [`BootNetwork`]：版本化配置（`ProxyDhcp` / `ExternalDhcp` / `RedfishOnly`）。
//! - [`DirectiveSnapshot`]：控制面经长轮询推送的全量应答指令快照。
//! - [`Observation`] 与 [`Heartbeat`]：bootnet 上报的观测与心跳。
//!
//! 本模块只定义数据与纯函数，不做 I/O。

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::decision::{BootDirective, DhcpRules, DirectiveTable, FirmwareName, UefiFirmware};
use crate::lifecycle::{SessionId, Timestamp};
use crate::value::{BootToken, HttpUrl, MacAddr, ParseError};
use crate::versioning::{ConfigDocument, ConfigError, ConfigKind, FieldError, field_error};

/// Linux 网卡名：1–15 字节，字母、数字、`.`、`_`、`-`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NetIface(String);

impl NetIface {
    /// 解析网卡名。
    ///
    /// # Errors
    ///
    /// 为空、超过 15 字节（`IFNAMSIZ - 1`）或含非法字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() {
            return Err(ParseError::new("NetIface", "empty"));
        }
        if raw.len() > 15 {
            return Err(ParseError::new("NetIface", "too_long"));
        }
        let ok = raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        if ok && raw != "." && raw != ".." {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("NetIface", "bad_format"))
        }
    }

    /// 网卡名文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for NetIface {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for NetIface {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for NetIface {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// 小写十六进制 SHA-256（64 字符）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha256Hex(String);

impl Sha256Hex {
    /// 解析摘要，大写会被规范为小写。
    ///
    /// # Errors
    ///
    /// 长度不是 64 或含非十六进制字符时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.len() != 64 {
            return Err(ParseError::new("Sha256Hex", "bad_length"));
        }
        if raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Ok(Self(raw.to_ascii_lowercase()))
        } else {
            Err(ParseError::new("Sha256Hex", "bad_format"))
        }
    }

    /// 由 32 字节摘要构造。
    #[must_use]
    pub fn from_digest(digest: &[u8; 32]) -> Self {
        Self(hex::encode(digest))
    }

    /// 摘要文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 前 12 个字符，用于 TFTP 路径 `fw/<sha12>/<名称>`。
    #[must_use]
    pub fn short12(&self) -> &str {
        self.0.get(..12).unwrap_or(&self.0)
    }
}

impl Display for Sha256Hex {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Sha256Hex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256Hex {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// 引导网络运行模式（Web 可改、版本化）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum BootNetwork {
    /// 本机做 `ProxyDHCP`（67/4011）并提供 TFTP（69）。
    ProxyDhcp {
        /// 监听网卡。
        iface: NetIface,
    },
    /// 复用现网 DHCP：只提供 TFTP（69）。
    ExternalDhcp {
        /// 监听网卡。
        iface: NetIface,
    },
    /// 只走 Redfish 虚拟介质：关闭全部监听。
    RedfishOnly,
}

impl BootNetwork {
    /// 监听网卡（`RedfishOnly` 为 `None`）。
    #[must_use]
    pub const fn iface(&self) -> Option<&NetIface> {
        match self {
            Self::ProxyDhcp { iface } | Self::ExternalDhcp { iface } => Some(iface),
            Self::RedfishOnly => None,
        }
    }

    /// 稳定的模式名。
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ProxyDhcp { .. } => "proxy_dhcp",
            Self::ExternalDhcp { .. } => "external_dhcp",
            Self::RedfishOnly => "redfish_only",
        }
    }

    /// 是否需要监听 67/4011。
    #[must_use]
    pub const fn serves_dhcp(&self) -> bool {
        matches!(self, Self::ProxyDhcp { .. })
    }

    /// 校验网卡在白名单内。
    ///
    /// # Errors
    ///
    /// 网卡不在 `allowed` 中时返回字段错误。
    pub fn check_allowed(&self, allowed: &[NetIface]) -> Result<(), Vec<FieldError>> {
        match self.iface() {
            Some(iface) if !allowed.contains(iface) => Err(vec![field_error(
                "/iface",
                "iface_not_allowed",
                "网卡不在 XPXE_BOOTNET_ALLOWED_IFACES 白名单内",
            )]),
            Some(_) | None => Ok(()),
        }
    }
}

impl ConfigDocument for BootNetwork {
    const KIND: ConfigKind = ConfigKind::BootNetwork;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        Ok(())
    }

    fn migrate(from_schema: u32, value: serde_json::Value) -> Result<Self, ConfigError> {
        crate::versioning::migrate_current(from_schema, value)
    }
}

/// 一组 iPXE 固件的 SHA-256。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirmwareSet {
    /// `undionly.kpxe`。
    pub undionly: Sha256Hex,
    /// `ipxe.efi`。
    pub ipxe_efi: Sha256Hex,
    /// `snponly.efi`。
    pub snponly: Sha256Hex,
}

impl FirmwareSet {
    /// 某个固件的摘要。
    #[must_use]
    pub const fn digest(&self, name: FirmwareName) -> &Sha256Hex {
        match name {
            FirmwareName::UndionlyKpxe => &self.undionly,
            FirmwareName::IpxeEfi => &self.ipxe_efi,
            FirmwareName::SnpOnlyEfi => &self.snponly,
        }
    }

    /// `ProxyDHCP` 下发的 TFTP 路径 `fw/<sha12>/<名称>`。
    #[must_use]
    pub fn tftp_path(&self, name: FirmwareName) -> String {
        format!("fw/{}/{}", self.digest(name).short12(), name.as_str())
    }

    /// 三个摘要。
    #[must_use]
    pub const fn digests(&self) -> [&Sha256Hex; 3] {
        [&self.undionly, &self.ipxe_efi, &self.snponly]
    }
}

/// 快照中的一条应答指令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotEntry {
    /// 目标 MAC。
    pub mac: MacAddr,
    /// 装机会话。
    pub session: SessionId,
    /// 引导令牌。
    pub boot_token: BootToken,
    /// 本会话使用的固件。
    pub firmware: FirmwareSet,
    /// 按节点覆盖 UEFI 固件（缺省用全局规则）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uefi_firmware: Option<UefiFirmware>,
    /// 过期时间；bootnet 侧再次检查。
    pub expires_at: Timestamp,
}

/// 控制面推送的全量指令快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectiveSnapshot {
    /// 单调递增的版本号。
    pub rev: u64,
    /// 运行模式。
    pub network: BootNetwork,
    /// 应答规则。
    pub rules: DhcpRules,
    /// 引导服务基址（含端口）。
    pub boot_base_url: HttpUrl,
    /// 当前激活的固件（TFTP 别名 `undionly.kpxe` 等指向它）；无激活版本时为空。
    #[serde(default)]
    pub active_firmware: Option<FirmwareSet>,
    /// 已武装节点。
    #[serde(default)]
    pub directives: Vec<SnapshotEntry>,
    /// 管理员已确认、可忽略的冲突服务器。
    #[serde(default)]
    pub acknowledged_conflicts: Vec<Ipv4Addr>,
}

impl DirectiveSnapshot {
    /// 不应答任何人的空快照（失联或启动时）。
    #[must_use]
    pub const fn closed(rev: u64, network: BootNetwork, boot_base_url: HttpUrl) -> Self {
        Self {
            rev,
            network,
            rules: DhcpRules::standard(),
            boot_base_url,
            active_firmware: None,
            directives: Vec::new(),
            acknowledged_conflicts: Vec::new(),
        }
    }

    /// 转为领域决策使用的指令表。
    #[must_use]
    pub fn table(&self) -> DirectiveTable {
        let entries = self
            .directives
            .iter()
            .map(|entry| {
                BootDirective::new(
                    entry.mac,
                    entry.session.clone(),
                    entry.boot_token.clone(),
                    entry.expires_at,
                )
            })
            .collect();
        DirectiveTable::new(self.boot_base_url.clone(), entries)
    }

    /// 按 MAC 查找指令。
    #[must_use]
    pub fn entry(&self, mac: MacAddr) -> Option<&SnapshotEntry> {
        self.directives.iter().find(|entry| entry.mac == mac)
    }

    /// 快照引用的全部固件摘要（去重）。
    #[must_use]
    pub fn referenced_firmware(&self) -> BTreeSet<Sha256Hex> {
        self.directives
            .iter()
            .map(|entry| &entry.firmware)
            .chain(self.active_firmware.iter())
            .flat_map(FirmwareSet::digests)
            .cloned()
            .collect()
    }

    /// 某 MAC 适用的应答规则（含按节点覆盖的 UEFI 固件）。
    #[must_use]
    pub fn rules_for(&self, mac: MacAddr) -> DhcpRules {
        self.entry(mac)
            .and_then(|entry| entry.uefi_firmware)
            .map_or(self.rules, |firmware| {
                self.rules.with_uefi_firmware(firmware)
            })
    }
}

/// 应答类型（观测用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyStage {
    /// 回应 67 端口 DISCOVER。
    Offer,
    /// 回应 4011 端口 REQUEST。
    Ack,
}

/// TFTP 传输阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferStage {
    /// 开始。
    Started,
    /// 完整传完。
    Completed,
    /// 失败（超时、对端错误、未知文件、拒绝）。
    Failed,
}

/// DHCP 请求摘要（观测用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestSummary {
    /// 客户端 MAC。
    pub mac: MacAddr,
    /// 入站端口（67 或 4011）。
    pub port: u16,
    /// 选项 93。
    pub arch: Option<u16>,
    /// 选项 60。
    pub vendor_class: Option<String>,
    /// 选项 77。
    pub user_class: Option<String>,
}

/// bootnet 上报的一条观测。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Observation {
    /// 发出了 Proxy OFFER/ACK。
    DhcpReply {
        /// 时间。
        at: Timestamp,
        /// 请求摘要。
        request: RequestSummary,
        /// 应答类型。
        stage: ReplyStage,
        /// 下发的引导文件（TFTP 路径或 URL）。
        file: String,
    },
    /// 未应答。
    DhcpIgnore {
        /// 时间。
        at: Timestamp,
        /// 请求摘要。
        request: RequestSummary,
        /// 原因（稳定蛇形名，如 `not_armed`）。
        reason: String,
    },
    /// TFTP 传输事件。
    Tftp {
        /// 时间。
        at: Timestamp,
        /// 客户端地址。
        client: IpAddr,
        /// 请求的文件。
        file: String,
        /// 阶段。
        stage: TransferStage,
        /// 已传字节。
        bytes: u64,
        /// 耗时（毫秒）。
        millis: u64,
    },
    /// 模式切换。
    ModeChanged {
        /// 时间。
        at: Timestamp,
        /// 旧模式。
        from: String,
        /// 新模式。
        to: String,
    },
    /// 现网 DHCP 探测结论。
    Probe {
        /// 时间。
        at: Timestamp,
        /// 结论。
        report: ProbeReport,
    },
}

/// 现网 DHCP 应答者分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponderClass {
    /// 只分配地址的正常 DHCP。
    AddressOnly,
    /// 分配地址且下发引导参数（`siaddr`/`file`/66/67/43）。
    PxeCapable,
    /// 另一台 `ProxyDHCP`（`yiaddr = 0` 且选项 60 = `PXEClient`）。
    Proxy,
}

/// 一个现网 DHCP 应答者。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Responder {
    /// 选项 54（缺失时取报文源地址）。
    pub server: Ipv4Addr,
    /// 分类。
    pub class: ResponderClass,
    /// `siaddr`。
    pub siaddr: Ipv4Addr,
    /// `file` 或选项 67。
    pub file: Option<String>,
    /// 应答携带的选项号。
    pub options: Vec<u8>,
}

/// `ExternalDhcp` 模式下对现网 DHCP 的检查结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalCheck {
    /// 已正确指向本机。
    Correct,
    /// 缺少 next-server（`siaddr` 不是本机）。
    MissingNextServer,
    /// 文件名不符。
    FileMismatch,
    /// 没有收到任何 DHCP 应答。
    NoDhcp,
}

/// 一次探测的结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeReport {
    /// 时间。
    pub at: Timestamp,
    /// 应答者。
    pub responders: Vec<Responder>,
    /// 与本机冲突且未被确认的服务器（`ProxyDhcp` 模式）。
    pub conflicts: Vec<Ipv4Addr>,
    /// `ExternalDhcp` 模式下的检查结论。
    pub external: Option<ExternalCheck>,
}

impl ProbeReport {
    /// 依据模式与已确认列表给出结论。
    #[must_use]
    pub fn assess(
        at: Timestamp,
        responders: Vec<Responder>,
        network: &BootNetwork,
        local: Ipv4Addr,
        acknowledged: &[Ipv4Addr],
    ) -> Self {
        let conflicts = if network.serves_dhcp() {
            responders
                .iter()
                .filter(|responder| responder.class != ResponderClass::AddressOnly)
                .map(|responder| responder.server)
                .filter(|server| *server != local && !acknowledged.contains(server))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        let external = matches!(network, BootNetwork::ExternalDhcp { .. })
            .then(|| external_check(&responders, local));
        Self {
            at,
            responders,
            conflicts,
            external,
        }
    }
}

/// `ExternalDhcp` 模式：现网 DHCP 是否指向本机、文件名是否为预期固件名。
fn external_check(responders: &[Responder], local: Ipv4Addr) -> ExternalCheck {
    let Some(first) = responders
        .iter()
        .find(|responder| responder.class != ResponderClass::Proxy)
    else {
        return ExternalCheck::NoDhcp;
    };
    if first.siaddr != local {
        return ExternalCheck::MissingNextServer;
    }
    let expected = [
        FirmwareName::UndionlyKpxe,
        FirmwareName::IpxeEfi,
        FirmwareName::SnpOnlyEfi,
    ];
    let matches = first.file.as_deref().is_some_and(|file| {
        expected
            .iter()
            .any(|name| file.trim_start_matches('/') == name.as_str())
    });
    if matches {
        ExternalCheck::Correct
    } else {
        ExternalCheck::FileMismatch
    }
}

/// bootnet 自报的健康状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    /// 正常。
    Ok,
    /// 降级：失联、端口占用等。
    Degraded,
    /// 检测到冲突的 DHCP 引导服务器，已暂停应答。
    Conflict,
}

/// 计数器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Counters {
    /// 收到的 DHCP 报文。
    pub dhcp_received: u64,
    /// 解析失败或超长丢弃。
    pub dhcp_malformed: u64,
    /// 限速丢弃。
    pub dhcp_rate_limited: u64,
    /// 发出的应答。
    pub dhcp_replies: u64,
    /// 不应答。
    pub dhcp_ignored: u64,
    /// 冲突暂停期间丢弃。
    pub dhcp_paused: u64,
    /// TFTP 完成。
    pub tftp_completed: u64,
    /// TFTP 失败或拒绝。
    pub tftp_failed: u64,
}

/// 心跳。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    /// 时间。
    pub at: Timestamp,
    /// bootnet 版本。
    pub version: String,
    /// 已应用的快照版本。
    pub rev: u64,
    /// 模式名。
    pub mode: String,
    /// 健康状态。
    pub health: Health,
    /// 降级原因。
    pub detail: Option<String>,
    /// 正在监听的 UDP 端口（67、4011、69 的子集）。
    pub listening: Vec<u16>,
    /// 计数器。
    pub counters: Counters,
    /// 最近一次探测。
    pub probe: Option<ProbeReport>,
}

/// `GET /internal/v1/bootnet/heartbeat` 的响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatRecord {
    /// 控制面收到心跳的时间。
    pub received_at: Timestamp,
    /// 心跳内容。
    pub heartbeat: Heartbeat,
}

impl HeartbeatRecord {
    /// 健康检查：最近一次心跳在 `max_age_secs` 内且自报 `ok`。
    #[must_use]
    pub const fn is_healthy(&self, now: Timestamp, max_age_secs: u64) -> bool {
        let age = now
            .unix_seconds()
            .saturating_sub(self.received_at.unix_seconds());
        age <= max_age_secs && matches!(self.heartbeat.health, Health::Ok)
    }
}

#[cfg(test)]
mod tests;
