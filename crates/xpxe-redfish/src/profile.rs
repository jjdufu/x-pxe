//! 厂商画像：`VirtualMedia` 位置、`InsertMedia` 字段、是否先弹出、启动覆盖方式、重启策略、任务轮询。

/// `VirtualMedia` 所在位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmediaLocation {
    /// `ComputerSystem.VirtualMedia`。
    System,
    /// `Managers[*].VirtualMedia`。
    Manager,
    /// 先找 System，再找 Manager。
    Auto,
}

/// `InsertMedia` 中的传输字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFields {
    /// 不携带 `TransferMethod` / `TransferProtocolType`。
    Omit,
    /// 只携带 `TransferProtocolType: HTTP`。
    ProtocolOnly,
    /// 携带 `TransferMethod: Stream` 与 `TransferProtocolType: HTTP`。
    MethodAndProtocol,
}

/// `InsertMedia` 请求体字段选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertFields {
    /// 传输字段。
    pub transfer: TransferFields,
    /// 是否携带 `WriteProtected: true`。
    pub write_protected: bool,
}

/// 启动覆盖方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootOverrideStyle {
    /// `PATCH ComputerSystem { Boot: {…} }`（DMTF 标准）。
    Standard,
}

/// `ComputerSystem.Reset` 的 `ResetType`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetType {
    /// 开机。
    On,
    /// 强制关机。
    ForceOff,
    /// 强制重启。
    ForceRestart,
    /// 正常重启。
    GracefulRestart,
}

impl ResetType {
    /// Redfish 取值。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::On => "On",
            Self::ForceOff => "ForceOff",
            Self::ForceRestart => "ForceRestart",
            Self::GracefulRestart => "GracefulRestart",
        }
    }
}

/// 重启策略：按当前电源状态选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetPolicy {
    /// 已开机时。
    pub when_on: ResetType,
    /// 已关机时。
    pub when_off: ResetType,
}

/// 厂商画像。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VendorProfile {
    /// `VirtualMedia` 位置。
    pub vmedia: VmediaLocation,
    /// `InsertMedia` 字段。
    pub insert_fields: InsertFields,
    /// 插入前总是先 `EjectMedia`（即使未报告已插入）。
    pub eject_before_insert: bool,
    /// 启动覆盖方式。
    pub boot_override: BootOverrideStyle,
    /// 重启策略。
    pub reset: ResetPolicy,
    /// 202 响应时按 `Location`（`TaskMonitor`）轮询到任务结束。
    pub task_polling: bool,
}

/// 内置画像标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileId {
    /// DMTF 标准（sushy-tools）。
    Generic,
    /// Dell iDRAC9。
    DellIdrac9,
    /// HPE iLO 5。
    HpeIlo5,
    /// HPE iLO 6。
    HpeIlo6,
    /// Supermicro X12。
    SupermicroX12,
    /// Lenovo XCC。
    LenovoXcc,
}

/// 默认重启策略。
const RESTART: ResetPolicy = ResetPolicy {
    when_on: ResetType::ForceRestart,
    when_off: ResetType::On,
};

impl ProfileId {
    /// 全部内置画像。
    pub const ALL: [Self; 6] = [
        Self::Generic,
        Self::DellIdrac9,
        Self::HpeIlo5,
        Self::HpeIlo6,
        Self::SupermicroX12,
        Self::LenovoXcc,
    ];

    /// 稳定名称。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::DellIdrac9 => "dell_idrac9",
            Self::HpeIlo5 => "hpe_ilo5",
            Self::HpeIlo6 => "hpe_ilo6",
            Self::SupermicroX12 => "supermicro_x12",
            Self::LenovoXcc => "lenovo_xcc",
        }
    }

    /// 按稳定名称查找。
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.as_str() == name)
    }

    /// 是否已在真机/模拟器上验证（未验证的在 Web 上标注）。
    #[must_use]
    pub const fn verified(self) -> bool {
        matches!(self, Self::Generic)
    }

    /// 按 `ServiceRoot.Vendor` 自动选择；未知厂商用 [`ProfileId::Generic`]。
    #[must_use]
    pub fn from_vendor(vendor: Option<&str>) -> Self {
        let lower = vendor.unwrap_or_default().to_ascii_lowercase();
        if lower.contains("dell") {
            Self::DellIdrac9
        } else if lower.contains("hpe") || lower.contains("hewlett") {
            Self::HpeIlo5
        } else if lower.contains("supermicro") {
            Self::SupermicroX12
        } else if lower.contains("lenovo") {
            Self::LenovoXcc
        } else {
            Self::Generic
        }
    }

    /// 画像内容。
    #[must_use]
    pub const fn profile(self) -> VendorProfile {
        let (vmedia, transfer, eject, task) = match self {
            Self::Generic => (VmediaLocation::Auto, TransferFields::Omit, false, true),
            Self::DellIdrac9 => (
                VmediaLocation::Auto,
                TransferFields::ProtocolOnly,
                true,
                true,
            ),
            Self::HpeIlo5 | Self::HpeIlo6 => {
                (VmediaLocation::Manager, TransferFields::Omit, true, false)
            }
            Self::SupermicroX12 => (
                VmediaLocation::Manager,
                TransferFields::ProtocolOnly,
                true,
                false,
            ),
            Self::LenovoXcc => (
                VmediaLocation::Manager,
                TransferFields::MethodAndProtocol,
                true,
                true,
            ),
        };
        VendorProfile {
            vmedia,
            insert_fields: InsertFields {
                transfer,
                write_protected: true,
            },
            eject_before_insert: eject,
            boot_override: BootOverrideStyle::Standard,
            reset: RESTART,
            task_polling: task,
        }
    }
}

#[cfg(test)]
mod tests;
