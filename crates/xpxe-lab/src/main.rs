//! `xpxe-lab`：实验室探针与依赖预检。
//!
//! 子命令：
//! - `deps-check`：对 workspace 选用的每个第三方依赖调用一次最小 API，确认它们在门禁下可用。
//! - `proxydhcp`：最小 `ProxyDHCP`（67/4011），只应答允许的 MAC。
//! - `tftp`：最小只读 TFTP，只服务列出的文件。
//! - `serve`：最小 HTTP 端（静态文件 + 事件 POST 记录）。
//! - `capture`：从 tcpdump 抓包中提取 DHCP 报文为夹具。
//! - `dhcp-scan`：列出实验网上所有应答的 DHCP 服务器。
//! - `pxe-client`：发出指定类型的 PXE DISCOVER（及 4011 REQUEST）。
//! - `iso`：生成 UEFI 引导 ISO。
//! - `render`：用内置预设与节点规格渲染装机输入（Step 4）。
//! - `render-plan`：用内置初始化预设渲染计划文件（Step 5）。
//! - `store-smoke`：存储层冒烟（建库、版本、回滚、审计、备份，Step 6）。
//! - `assets-smoke`：联网获取并校验资产（Step 7）。
//! - `assets-align`：netboot 内核 ABI 与镜像源对齐检查（Step 7）。
//!
//! 发 DHCP 的子命令只在带 10.77.0.0/24 直连路由且不承载默认路由的网卡上运行。

mod assets_smoke;
mod capture;
mod common;
mod control_stub;
mod deps_check;
mod iso;
mod logging;
mod node_check;
mod proxydhcp;
mod redfish;
mod render;
mod render_plan;
mod scan;
mod serve;
mod store_smoke;
mod tftp;

use std::io::Write as _;
use std::process::ExitCode;

use common::{Args, LabError, LabResult};

/// 用法说明。
const USAGE: &str = "用法：xpxe-lab <deps-check|proxydhcp|tftp|control-stub|serve|capture|dhcp-scan|pxe-client|iso|redfish|node-check|render|render-plan|store-smoke|assets-smoke|assets-align> [参数…]";

/// 运行依赖预检并打印报告。
///
/// # Errors
///
/// 预检失败时返回 [`LabError`]。
fn deps() -> LabResult {
    let report = deps_check::run().map_err(|error| LabError(error.to_string()))?;
    let mut stdout = std::io::stdout().lock();
    for line in report {
        writeln!(stdout, "deps-check: {line} ok")?;
    }
    writeln!(stdout, "deps-check: 全部通过")?;
    Ok(())
}

/// 进程入口：按第一个参数分派子命令。
fn main() -> ExitCode {
    let mut rest = std::env::args().skip(1);
    let command = rest.next().unwrap_or_default();
    let args = Args::new(rest.collect());
    let outcome = match command.as_str() {
        "deps-check" => deps(),
        "proxydhcp" => proxydhcp::run(&args),
        "tftp" => tftp::run(&args),
        "control-stub" => control_stub::run(&args),
        "redfish" => redfish::run(&args),
        "node-check" => node_check::run(&args),
        "serve" => serve::run(&args),
        "capture" => capture::run(&args),
        "dhcp-scan" => scan::run_scan(&args),
        "pxe-client" => scan::run_pxe(&args),
        "iso" => iso::run(&args),
        "render" => render::run(&args),
        "render-plan" => render_plan::run(&args),
        "store-smoke" => store_smoke::run(&args),
        "assets-smoke" => assets_smoke::run_smoke(&args),
        "assets-align" => assets_smoke::run_align(&args),
        _ => Err(LabError(USAGE.to_owned())),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let mut stderr = std::io::stderr().lock();
            if writeln!(stderr, "xpxe-lab: {error}").is_err() {
                return ExitCode::from(2);
            }
            ExitCode::FAILURE
        }
    }
}
