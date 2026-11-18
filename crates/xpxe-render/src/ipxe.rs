//! iPXE 脚本。

use serde::Serialize;

use crate::RenderError;
use crate::request::RenderRequest;
use crate::template::render_named;

/// 武装节点的引导脚本。
const ARMED: &str = include_str!("templates/boot-armed.ipxe.j2");
/// 非武装节点：打印原因后退出，避免循环。
const NOT_ARMED: &str = include_str!("templates/boot-exit.ipxe.j2");

/// 渲染入口脚本。
///
/// # Errors
///
/// 模板渲染失败时返回错误。
pub(crate) fn render_ipxe(request: &RenderRequest, cmdline: &str) -> Result<String, RenderError> {
    if request.armed() {
        render_named(
            "boot-armed.ipxe",
            ARMED,
            &ArmedContext {
                boot_base: request.boot_base().as_str(),
                boot_token: request.boot_token().as_str(),
                hostname: request.node().hostname().as_str(),
                mac: request.node().mac().to_colon(),
                session_short: request.session_short(),
                cmdline,
                firmware_cpio: request.profile().firmware_cpio(),
            },
        )
    } else {
        render_named(
            "boot-exit.ipxe",
            NOT_ARMED,
            &ExitContext {
                reason: request.not_armed_reason(),
            },
        )
    }
}

/// 武装脚本上下文。
#[derive(Serialize)]
struct ArmedContext<'input> {
    /// HTTP 根。
    boot_base: &'input str,
    /// 令牌。
    boot_token: &'input str,
    /// 主机名。
    hostname: &'input str,
    /// MAC。
    mac: String,
    /// 会话短标识。
    session_short: &'input str,
    /// 内核参数。
    cmdline: &'input str,
    /// 是否追加 firmware.cpio.gz。
    firmware_cpio: bool,
}

/// 退出脚本上下文。
#[derive(Serialize)]
struct ExitContext<'input> {
    /// 原因。
    reason: &'input str,
}
