//! `xpxe-control` 二进制入口。

use std::io::Write as _;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;
use xpxe_control::cli;

/// 进程入口：解析命令并在多线程运行时上执行。
fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_env("XPXE_LOG").unwrap_or_else(|_unset| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = cli::parse(&args).and_then(|command| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?
            .block_on(cli::run(command))
    });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _written = writeln!(std::io::stderr().lock(), "{message}");
            ExitCode::FAILURE
        }
    }
}
