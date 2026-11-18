//! 宿主副作用：权限检查、禁用服务、重启、睡眠与时钟。

use std::process::Command;
use std::time::{Duration, Instant};

/// 由测试替换的宿主操作。
pub trait Host {
    /// 是否以 UID 0 运行。
    fn is_root(&self) -> bool;
    /// 禁用 systemd 单元。
    ///
    /// # Errors
    ///
    /// 命令失败时返回说明。
    fn disable_service(&mut self, unit: &str) -> Result<(), String>;
    /// 重启。
    ///
    /// # Errors
    ///
    /// 命令失败时返回说明。
    fn reboot(&mut self) -> Result<(), String>;
    /// 睡眠。
    fn sleep(&mut self, duration: Duration);
    /// 单调时钟。
    fn now(&self) -> Instant;
}

/// 真实宿主。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemHost;

/// 运行 `systemctl`。
///
/// # Errors
///
/// 无法启动或非零退出时返回说明。
fn systemctl(args: &[&str]) -> Result<(), String> {
    let status = Command::new("systemctl")
        .args(args)
        .status()
        .map_err(|error| format!("systemctl {}：{error}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl {} 退出 {status}", args.join(" ")))
    }
}

impl Host for SystemHost {
    fn is_root(&self) -> bool {
        std::fs::read_to_string("/proc/self/status").is_ok_and(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|rest| rest.split_whitespace().nth(1))
                == Some("0")
        })
    }

    fn disable_service(&mut self, unit: &str) -> Result<(), String> {
        systemctl(&["disable", unit])
    }

    fn reboot(&mut self) -> Result<(), String> {
        systemctl(&["reboot"])
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}
