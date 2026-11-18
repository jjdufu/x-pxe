//! `xpxe-bootnet` 二进制入口：`run`（常驻）、`healthcheck`（容器健康检查）、`--version`。

use std::io::Write as _;
use std::process::ExitCode;

use xpxe_bootnet::BootnetError;
use xpxe_bootnet::daemon::{RunOptions, healthcheck, run};
use xpxe_bootnet::settings::Settings;

/// 写一行到标准错误（失败时无处可报，返回值只用于决定退出码）。
fn report(line: &str) -> ExitCode {
    let mut stderr = std::io::stderr().lock();
    if writeln!(stderr, "xpxe-bootnet: {line}").is_ok() {
        ExitCode::FAILURE
    } else {
        ExitCode::from(2)
    }
}

/// 初始化 JSON 日志（`XPXE_LOG`，默认 `info`）。
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("XPXE_LOG")
        .unwrap_or_else(|_error| tracing_subscriber::EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .finish();
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        let _code = report("日志初始化失败");
    }
}

/// 建立运行时并执行子命令。
///
/// # Errors
///
/// 配置、运行时或子命令失败时返回错误。
fn dispatch(command: &str) -> Result<ExitCode, BootnetError> {
    let settings = Settings::from_env()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| BootnetError::new("runtime", error.to_string()))?;
    match command {
        "run" => {
            init_logging();
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "xpxe-bootnet 启动");
            runtime.block_on(run(&settings, RunOptions::default()))?;
            Ok(ExitCode::SUCCESS)
        }
        "healthcheck" => {
            let healthy = runtime.block_on(healthcheck(&settings))?;
            Ok(if healthy {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        other => Err(BootnetError::new(
            "usage",
            format!("未知子命令 {other}；用法：xpxe-bootnet run|healthcheck|--version"),
        )),
    }
}

/// 进程入口。
fn main() -> ExitCode {
    let command = std::env::args().nth(1).unwrap_or_default();
    if command == "--version" {
        let mut stdout = std::io::stdout().lock();
        return if writeln!(
            stdout,
            "{} {}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        )
        .is_ok()
        {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    match dispatch(&command) {
        Ok(code) => code,
        Err(error) => report(&error.to_string()),
    }
}
