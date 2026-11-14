//! `xpxe-bootnet` 二进制入口（骨架，只输出版本）。

use std::io::Write as _;
use std::process::ExitCode;

/// 进程入口：输出名称与版本。
fn main() -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    if writeln!(
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
    }
}
