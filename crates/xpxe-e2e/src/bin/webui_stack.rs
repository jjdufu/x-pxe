//! `webui-stack`：启动 `WebUI` 联调栈并等待 Ctrl-C（见 `xpxe_e2e::stack`）。

use std::io::Write as _;
use std::path::Path;

use xpxe_e2e::Outcome;
use xpxe_e2e::stack::{Stack, config_from};

/// 启动、打印连接信息、等待退出信号。
///
/// # Errors
///
/// 启动失败时返回错误。
async fn serve() -> Outcome {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = config_from(&args, &root)?;
    let stack = Stack::start(&config).await?;
    let info = stack.info();
    std::fs::write(config.dir.join("stack.json"), info.to_string())?;
    {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "webui-stack ready {info}")?;
        stdout.flush()?;
    }
    tokio::signal::ctrl_c().await?;
    stack.shutdown().await;
    Ok(())
}

/// 进程入口。
fn main() -> std::process::ExitCode {
    let built = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build();
    let outcome = match built {
        Ok(runtime) => runtime.block_on(serve()),
        Err(error) => Err(error.into()),
    };
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let mut stderr = std::io::stderr().lock();
            let _written = writeln!(stderr, "webui-stack: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
