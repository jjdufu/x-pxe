//! 内核模块：sysctl 与 limits。

use serde::{Deserialize, Serialize};

use super::types::{Flag, Ranged, SafeFileName, SysctlKey, SysctlValue};

/// 缓冲区字节数上限（1 GiB）。
const BUF_MAX: u64 = 1 << 30;
/// 计数类参数上限（2^31-1）。
const COUNT_MAX: u64 = 2_147_483_647;

/// 缓冲区大小。
pub type BufSize = Ranged<4096, BUF_MAX>;
/// 计数。
pub type Count = Ranged<1, COUNT_MAX>;

/// 三档预设（Web 用来一键填充参数；保存的是参数本身）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SysctlPreset {
    /// `legacy/init-d13.sh` 的内核参数（逐项一致：转发开启、关闭 IPv6）。
    #[default]
    Deb13InitScripts,
    /// Web 服务器（不转发、保留 IPv6）。
    WebServer,
    /// 最小（接近发行版默认，仅保留连接表与文件监控调优）。
    Minimal,
}

/// 转发与 IPv6。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForwardParams {
    /// `net.ipv4.ip_forward`。
    pub ip_forward: Flag,
    /// `net.ipv6.conf.{all,default,lo}.disable_ipv6`。
    pub disable_ipv6: Flag,
}

/// TCP。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TcpParams {
    /// `tcp_syncookies`。
    pub syncookies: Flag,
    /// `tcp_tw_reuse`。
    pub tw_reuse: Ranged<0, 2>,
    /// `tcp_fin_timeout`。
    pub fin_timeout: Ranged<5, 120>,
    /// `tcp_syn_retries`。
    pub syn_retries: Ranged<1, 10>,
    /// `tcp_synack_retries`。
    pub synack_retries: Ranged<1, 10>,
    /// `tcp_fastopen`。
    pub fastopen: Ranged<0, 3>,
    /// `tcp_slow_start_after_idle`。
    pub slow_start_after_idle: Flag,
}

/// keepalive。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeepaliveParams {
    /// `tcp_keepalive_time`。
    pub time: Ranged<60, 7200>,
    /// `tcp_keepalive_intvl`。
    pub intvl: Ranged<10, 300>,
    /// `tcp_keepalive_probes`。
    pub probes: Ranged<1, 20>,
}

/// min/default/max 三元组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BufTriple {
    /// 最小。
    pub min: BufSize,
    /// 默认。
    pub default: BufSize,
    /// 最大。
    pub max: BufSize,
}

impl BufTriple {
    /// 内置值构造。
    const fn of(min: u64, default: u64, max: u64) -> Self {
        Self {
            min: Ranged::clamped(min),
            default: Ranged::clamped(default),
            max: Ranged::clamped(max),
        }
    }

    /// 是否满足 min ≤ default ≤ max。
    #[must_use]
    pub fn is_ordered(&self) -> bool {
        self.min <= self.default && self.default <= self.max
    }
}

/// 缓冲。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BufferParams {
    /// `tcp_rmem`。
    pub tcp_rmem: BufTriple,
    /// `tcp_wmem`。
    pub tcp_wmem: BufTriple,
    /// `net.core.rmem_default`。
    pub rmem_default: BufSize,
    /// `net.core.wmem_default`。
    pub wmem_default: BufSize,
    /// `net.core.rmem_max`。
    pub rmem_max: BufSize,
    /// `net.core.wmem_max`。
    pub wmem_max: BufSize,
}

/// 本地端口范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortRange {
    /// 下界。
    pub low: Ranged<1024, 65535>,
    /// 上界。
    pub high: Ranged<1024, 65535>,
}

/// 连接表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnTableParams {
    /// `tcp_max_orphans`。
    pub tcp_max_orphans: Count,
    /// `tcp_max_syn_backlog`。
    pub tcp_max_syn_backlog: Count,
    /// `tcp_max_tw_buckets`。
    pub tcp_max_tw_buckets: Count,
    /// `ip_local_port_range`。
    pub ip_local_port_range: PortRange,
}

/// 拥塞控制与队列规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Congestion {
    /// 不写，沿用系统默认。
    #[default]
    SystemDefault,
    /// `bbr` + `fq`。
    Bbr,
    /// `cubic` + `fq_codel`。
    Cubic,
}

/// 队列。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueParams {
    /// `net.core.netdev_max_backlog`。
    pub netdev_max_backlog: Count,
    /// `net.core.somaxconn`。
    pub somaxconn: Count,
}

/// 内存。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryParams {
    /// `vm.max_map_count`。
    pub max_map_count: Count,
    /// `vm.dirty_background_ratio`。
    pub dirty_background_ratio: Ranged<1, 50>,
    /// `vm.dirty_ratio`。
    pub dirty_ratio: Ranged<1, 90>,
    /// `vm.overcommit_memory`。
    pub overcommit_memory: Ranged<0, 2>,
    /// `vm.swappiness`。
    pub swappiness: Ranged<0, 100>,
    /// `vm.min_free_kbytes`。
    pub min_free_kbytes: Count,
    /// `vm.vfs_cache_pressure`。
    pub vfs_cache_pressure: Ranged<1, 1000>,
}

/// 文件监控。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchParams {
    /// `fs.fanotify.max_queued_events`。
    pub fanotify_max_queued_events: Count,
    /// `fs.inotify.max_user_watches`。
    pub inotify_max_user_watches: Count,
}

/// 安全。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityParams {
    /// `accept_redirects`（all/default）。
    pub accept_redirects: Flag,
    /// `secure_redirects`（all/default）。
    pub secure_redirects: Flag,
    /// `rp_filter`（all/default）。
    pub rp_filter: Ranged<0, 2>,
}

/// 全部有类型 sysctl 参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SysctlParams {
    /// 转发与 IPv6。
    pub forward: ForwardParams,
    /// TCP。
    pub tcp: TcpParams,
    /// keepalive。
    pub keepalive: KeepaliveParams,
    /// 缓冲。
    pub buffers: BufferParams,
    /// 连接表。
    pub conn_table: ConnTableParams,
    /// 拥塞。
    pub congestion: Congestion,
    /// 队列。
    pub queues: QueueParams,
    /// 内存。
    pub memory: MemoryParams,
    /// 文件监控。
    pub watch: WatchParams,
    /// 安全。
    pub security: SecurityParams,
}

impl Default for SysctlParams {
    fn default() -> Self {
        Self::for_preset(SysctlPreset::Deb13InitScripts)
    }
}

impl SysctlParams {
    /// 某预设的参数组合。
    #[must_use]
    pub const fn for_preset(preset: SysctlPreset) -> Self {
        let legacy = matches!(preset, SysctlPreset::Deb13InitScripts);
        let tuned = !matches!(preset, SysctlPreset::Minimal);
        Self {
            forward: ForwardParams {
                ip_forward: Flag::new(legacy),
                disable_ipv6: Flag::new(legacy),
            },
            tcp: TcpParams {
                syncookies: Flag::ON,
                tw_reuse: Ranged::clamped(if tuned { 1 } else { 2 }),
                fin_timeout: Ranged::clamped(if tuned { 30 } else { 60 }),
                syn_retries: Ranged::clamped(if tuned { 5 } else { 6 }),
                synack_retries: Ranged::clamped(5),
                fastopen: Ranged::clamped(if tuned { 3 } else { 1 }),
                slow_start_after_idle: Flag::new(!tuned),
            },
            keepalive: KeepaliveParams {
                time: Ranged::clamped(if tuned { 600 } else { 7200 }),
                intvl: Ranged::clamped(if tuned { 30 } else { 75 }),
                probes: Ranged::clamped(if tuned { 5 } else { 9 }),
            },
            buffers: Self::buffers(tuned),
            conn_table: ConnTableParams {
                tcp_max_orphans: Ranged::clamped(262_144),
                tcp_max_syn_backlog: Ranged::clamped(32_768),
                tcp_max_tw_buckets: Ranged::clamped(1_048_576),
                ip_local_port_range: PortRange {
                    low: Ranged::clamped(if tuned { 10_000 } else { 32_768 }),
                    high: Ranged::clamped(if tuned { 65_535 } else { 60_999 }),
                },
            },
            congestion: Congestion::SystemDefault,
            queues: QueueParams {
                netdev_max_backlog: Ranged::clamped(if tuned { 32_768 } else { 1000 }),
                somaxconn: Ranged::clamped(if tuned { 32_768 } else { 4096 }),
            },
            memory: Self::memory(tuned),
            watch: WatchParams {
                fanotify_max_queued_events: Ranged::clamped(65_536),
                inotify_max_user_watches: Ranged::clamped(524_288),
            },
            security: SecurityParams {
                accept_redirects: Flag::OFF,
                secure_redirects: Flag::OFF,
                rp_filter: Ranged::clamped(if legacy { 0 } else { 2 }),
            },
        }
    }

    /// 缓冲参数。
    const fn buffers(tuned: bool) -> BufferParams {
        let max = if tuned { 12_582_912 } else { 212_992 };
        BufferParams {
            tcp_rmem: BufTriple::of(4096, 87_380, if tuned { 12_582_912 } else { 6_291_456 }),
            tcp_wmem: BufTriple::of(4096, 16_384, if tuned { 12_582_912 } else { 4_194_304 }),
            rmem_default: Ranged::clamped(212_992),
            wmem_default: Ranged::clamped(if tuned { 16_384 } else { 212_992 }),
            rmem_max: Ranged::clamped(max),
            wmem_max: Ranged::clamped(max),
        }
    }

    /// 内存参数。
    const fn memory(tuned: bool) -> MemoryParams {
        MemoryParams {
            max_map_count: Ranged::clamped(if tuned { 262_144 } else { 65_530 }),
            dirty_background_ratio: Ranged::clamped(if tuned { 5 } else { 10 }),
            dirty_ratio: Ranged::clamped(if tuned { 10 } else { 20 }),
            overcommit_memory: Ranged::clamped(if tuned { 1 } else { 0 }),
            swappiness: Ranged::clamped(if tuned { 10 } else { 60 }),
            min_free_kbytes: Ranged::clamped(65_536),
            vfs_cache_pressure: Ranged::clamped(if tuned { 50 } else { 100 }),
        }
    }
}

/// 额外的 sysctl 键值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SysctlExtra {
    /// 键。
    pub key: SysctlKey,
    /// 值。
    pub value: SysctlValue,
}

/// 模块 15：sysctl。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sysctl {
    /// `/etc/sysctl.d/` 下的文件名。
    pub file_name: SafeFileName,
    /// 最近一次套用的预设（仅供展示）。
    pub preset: SysctlPreset,
    /// 参数。
    pub params: SysctlParams,
    /// 额外键值。
    pub extra: Vec<SysctlExtra>,
    /// 立即 `sysctl --system`。
    pub apply_now: Flag,
}

impl Default for Sysctl {
    fn default() -> Self {
        Self {
            file_name: SafeFileName::from_static("99-fleet-netvm.conf"),
            preset: SysctlPreset::Deb13InitScripts,
            params: SysctlParams::default(),
            extra: Vec::new(),
            apply_now: Flag::ON,
        }
    }
}

/// limits 取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum LimitValue {
    /// `unlimited`。
    #[default]
    Unlimited,
    /// 具体数值。
    Count(Count),
}

/// 模块 16：limits。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// `/etc/security/limits.d/` 下的文件名。
    pub file_name: SafeFileName,
    /// nofile（soft/hard）。
    pub nofile: Ranged<1024, 1_048_576>,
    /// nproc。
    pub nproc: LimitValue,
    /// core。
    pub core: LimitValue,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            file_name: SafeFileName::from_static("90-nofile.conf"),
            nofile: Ranged::clamped(655_350),
            nproc: LimitValue::Unlimited,
            core: LimitValue::Unlimited,
        }
    }
}
