//! `xpxe-bootnet`：特权引导网络进程。
//!
//! 只做三件事：`ProxyDHCP`（67/4011，只给引导信息、不分配地址）、只读 TFTP（69，只服务内存里的固件）、
//! 现网 DHCP 探测。应答内容完全来自控制面推送的指令快照（[`xpxe_domain::DirectiveSnapshot`]），
//! 不读任何配置文件；与控制面失联 30 s 即清空指令表（失败即关闭）。
//!
//! - [`settings`]：环境变量。
//! - [`upstream`]：到控制面内部 API 的 HTTP 客户端。
//! - [`answering`]：单个 DHCP 报文的处理（限速、解码、领域决策、编码）。
//! - [`failsafe`]：失联判定。
//! - [`memory`]：固件内存缓存与 TFTP 文件表。
//! - [`state`]：计数器、观测队列、健康状态。
//! - [`listeners`]：按模式开关 67/4011/69 监听。
//! - [`scan`]：现网 DHCP 探测。
//! - [`netinfo`]：网卡地址、MTU、内核版本。
//! - [`daemon`]：把以上装配成常驻进程，以及 `healthcheck`。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

use std::fmt::{Display, Formatter};

pub mod answering;
pub mod daemon;
pub mod failsafe;
pub mod listeners;
pub mod memory;
pub mod netinfo;
pub mod scan;
pub mod settings;
pub mod state;
pub mod upstream;

/// bootnet 的错误：稳定的类别码 + 说明（不含令牌等机密）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootnetError {
    /// 类别码，例如 `config`、`control`、`socket`。
    code: &'static str,
    /// 说明。
    message: String,
}

impl BootnetError {
    /// 构造错误。
    #[must_use]
    pub fn new<M: Into<String>>(code: &'static str, message: M) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 类别码。
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// 说明。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for BootnetError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BootnetError {}

/// 当前 Unix 时间（秒）。
#[must_use]
pub fn wall_clock() -> xpxe_domain::Timestamp {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    xpxe_domain::Timestamp::from_unix_seconds(secs)
}
