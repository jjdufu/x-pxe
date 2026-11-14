//! 节点规格：固件模式、引导通道与网络。

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::value::{Fqdn, Hostname, Ipv4Cidr, MacAddr, ParseError};
use crate::versioning::{FieldError, field_error};

/// 目标操作系统族。目前仅 Debian 13。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OsFamily {
    /// Debian 13（trixie）。
    Debian13,
}

/// 固件模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareMode {
    /// UEFI。
    Uefi,
    /// 传统 BIOS。
    Bios,
}

/// BMC 对象标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BmcId(String);

impl BmcId {
    /// 解析标识：1–64 位字母、数字、`-`、`_`。
    ///
    /// # Errors
    ///
    /// 格式不合法时返回错误。
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        if raw.is_empty() {
            return Err(ParseError::new("BmcId", "empty"));
        }
        if raw.len() > 64 {
            return Err(ParseError::new("BmcId", "too_long"));
        }
        let ok = raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseError::new("BmcId", "bad_format"))
        }
    }

    /// 标识文本。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BmcId {
    type Error = ParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<BmcId> for String {
    fn from(value: BmcId) -> Self {
        value.0
    }
}

impl std::fmt::Display for BmcId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 引导通道。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BootChannel {
    /// 局域网 PXE。
    Lan,
    /// Redfish 虚拟介质（仅 UEFI）。
    Redfish {
        /// 关联的 BMC。
        bmc: BmcId,
    },
}

/// 节点网络配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeNetwork {
    /// 由现网 DHCP 分配地址。
    Dhcp,
    /// 静态 IPv4。
    Static {
        /// 地址与前缀。
        address: Ipv4Cidr,
        /// 默认网关。
        gateway: Option<Ipv4Addr>,
        /// DNS 服务器。
        dns: Vec<Ipv4Addr>,
    },
}

/// 尚未校验跨字段约束的节点规格字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSpecParts {
    /// 主机名。
    pub hostname: Hostname,
    /// 可选 FQDN。
    pub fqdn: Option<Fqdn>,
    /// 管理用 MAC。
    pub mac: MacAddr,
    /// 操作系统族。
    pub os_family: OsFamily,
    /// 固件模式。
    pub firmware: FirmwareMode,
    /// 引导通道。
    pub channel: BootChannel,
    /// 网络配置。
    pub network: NodeNetwork,
}

/// 节点规格。跨字段约束在 [`NodeSpec::check_constraints`] 中检查。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NodeSpecParts", into = "NodeSpecParts")]
pub struct NodeSpec {
    /// 主机名。
    hostname: Hostname,
    /// 可选 FQDN。
    fqdn: Option<Fqdn>,
    /// 管理用 MAC。
    mac: MacAddr,
    /// 操作系统族。
    os_family: OsFamily,
    /// 固件模式。
    firmware: FirmwareMode,
    /// 引导通道。
    channel: BootChannel,
    /// 网络配置。
    network: NodeNetwork,
}

impl NodeSpec {
    /// 由字段组构造并校验规格。
    ///
    /// # Errors
    ///
    /// 跨字段约束失败时返回字段级错误。
    pub fn new(parts: NodeSpecParts) -> Result<Self, Vec<FieldError>> {
        let spec = Self::from_parts(parts);
        spec.check_constraints()?;
        Ok(spec)
    }

    /// 不加校验地由字段组填充（仅供校验前使用）。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    fn from_parts(parts: NodeSpecParts) -> Self {
        Self {
            hostname: parts.hostname,
            fqdn: parts.fqdn,
            mac: parts.mac,
            os_family: parts.os_family,
            firmware: parts.firmware,
            channel: parts.channel,
            network: parts.network,
        }
    }

    /// 拆回字段组。
    #[must_use]
    pub fn into_parts(self) -> NodeSpecParts {
        NodeSpecParts {
            hostname: self.hostname,
            fqdn: self.fqdn,
            mac: self.mac,
            os_family: self.os_family,
            firmware: self.firmware,
            channel: self.channel,
            network: self.network,
        }
    }

    /// 跨字段约束：Redfish 仅允许 UEFI，BIOS 只能走局域网 PXE。
    ///
    /// # Errors
    ///
    /// 违反约束时返回 `channel: redfish_requires_uefi`。
    pub fn check_constraints(&self) -> Result<(), Vec<FieldError>> {
        match (&self.firmware, &self.channel) {
            (FirmwareMode::Bios, BootChannel::Redfish { .. }) => Err(vec![field_error(
                "/channel",
                "redfish_requires_uefi",
                "BIOS 节点只能使用局域网 PXE，Redfish 虚拟介质需要 UEFI",
            )]),
            (FirmwareMode::Uefi | FirmwareMode::Bios, BootChannel::Lan)
            | (FirmwareMode::Uefi, BootChannel::Redfish { .. }) => Ok(()),
        }
    }

    /// 主机名。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn hostname(&self) -> &Hostname {
        &self.hostname
    }

    /// 可选 FQDN。
    #[must_use]
    pub const fn fqdn(&self) -> Option<&Fqdn> {
        self.fqdn.as_ref()
    }

    /// 管理用 MAC。
    #[must_use]
    pub const fn mac(&self) -> MacAddr {
        self.mac
    }

    /// 操作系统族。
    #[must_use]
    pub const fn os_family(&self) -> OsFamily {
        self.os_family
    }

    /// 固件模式。
    #[must_use]
    pub const fn firmware(&self) -> FirmwareMode {
        self.firmware
    }

    /// 引导通道。
    #[must_use]
    pub const fn channel(&self) -> &BootChannel {
        &self.channel
    }

    /// 网络配置。
    #[must_use]
    pub const fn network(&self) -> &NodeNetwork {
        &self.network
    }
}

impl TryFrom<NodeSpecParts> for NodeSpec {
    type Error = crate::versioning::ValidationError;

    fn try_from(parts: NodeSpecParts) -> Result<Self, Self::Error> {
        Self::new(parts).map_err(crate::versioning::ValidationError::from_fields)
    }
}

impl From<NodeSpec> for NodeSpecParts {
    fn from(value: NodeSpec) -> Self {
        value.into_parts()
    }
}

impl crate::versioning::ConfigDocument for NodeSpec {
    const KIND: crate::versioning::ConfigKind = crate::versioning::ConfigKind::NodeSpec;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        self.check_constraints()
    }

    fn migrate(
        from_schema: u32,
        value: serde_json::Value,
    ) -> Result<Self, crate::versioning::ConfigError> {
        crate::versioning::migrate_current(from_schema, value)
    }
}
