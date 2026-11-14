//! `xpxe-lab`：实验室探针与依赖预检。
//!
//! 子命令：
//! - `deps-check`：对 workspace 选用的每个第三方依赖调用一次最小 API，确认它们在门禁下可用。

mod deps_check;

use std::io::Write as _;
use std::process::ExitCode;

/// 进程入口：按第一个参数分派子命令。
fn main() -> ExitCode {
    let command = std::env::args().nth(1).unwrap_or_default();
    let outcome = match command.as_str() {
        "deps-check" => deps_check::run(),
        _ => Err(deps_check::CheckError::Usage),
    };
    let mut stdout = std::io::stdout().lock();
    match outcome {
        Ok(report) => {
            let written = report
                .iter()
                .try_for_each(|line| writeln!(stdout, "deps-check: {line} ok"));
            if written.is_ok() && writeln!(stdout, "deps-check: 全部通过").is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            let mut stderr = std::io::stderr().lock();
            if writeln!(stderr, "xpxe-lab: {error}").is_err() {
                return ExitCode::from(2);
            }
            ExitCode::FAILURE
        }
    }
}
