//! `xpxe-agent` 命令行：`run --plan <file>` / `verify --plan <file>`。

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use xpxe_agent::{AgentConfig, HttpReporter, RunOutcome, SystemHost, load_plan, run};

/// 用法。
const USAGE: &str = "用法：xpxe-agent run --plan <plan.json> [--shell <path>] [--state-dir <dir>]\n      xpxe-agent verify --plan <plan.json>";

/// 解析出的命令。
struct Cli {
    /// 子命令。
    command: String,
    /// 计划路径。
    plan: PathBuf,
    /// 可选 shell。
    shell: Option<PathBuf>,
    /// 可选状态目录。
    state_dir: Option<PathBuf>,
}

/// 解析参数。
///
/// # Errors
///
/// 参数或执行出错时返回说明。
fn parse<I: Iterator<Item = String>>(mut args: I) -> Result<Cli, String> {
    let command = args.next().ok_or_else(|| USAGE.to_owned())?;
    let mut plan_path = None;
    let mut shell = None;
    let mut state_dir = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} 缺少取值"))?;
        match flag.as_str() {
            "--plan" => plan_path = Some(PathBuf::from(value)),
            "--shell" => shell = Some(PathBuf::from(value)),
            "--state-dir" => state_dir = Some(PathBuf::from(value)),
            _ => return Err(format!("未知参数 {flag}\n{USAGE}")),
        }
    }
    let plan = plan_path.ok_or_else(|| format!("缺少 --plan\n{USAGE}"))?;
    Ok(Cli {
        command,
        plan,
        shell,
        state_dir,
    })
}

/// 执行 `run`。
///
/// # Errors
///
/// 参数或执行出错时返回说明。
fn run_command(cli: Cli) -> Result<ExitCode, String> {
    let mut config = AgentConfig::system(cli.plan);
    if let Some(shell) = cli.shell {
        config.shell = shell;
    }
    if let Some(dir) = cli.state_dir {
        config.state_dir = dir;
    }
    let plan = load_plan(&config.plan_path).map_err(|error| error.to_string())?;
    let mut reporter = HttpReporter::new(&plan.events_url)?;
    let outcome =
        run(&config, &mut SystemHost, &mut reporter).map_err(|error| error.to_string())?;
    Ok(match outcome {
        RunOutcome::Finished { .. } => ExitCode::SUCCESS,
        RunOutcome::Failed { .. } => ExitCode::from(1),
        RunOutcome::Undelivered { .. } => ExitCode::from(3),
    })
}

/// 分派子命令。
///
/// # Errors
///
/// 参数或执行出错时返回说明。
fn dispatch(cli: Cli) -> Result<ExitCode, String> {
    match cli.command.as_str() {
        "run" => run_command(cli),
        "verify" => {
            let plan = load_plan(&cli.plan).map_err(|error| error.to_string())?;
            let _out = writeln!(
                std::io::stdout(),
                "计划有效：会话 {}，{} 个模块",
                plan.session,
                plan.modules.len()
            );
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("未知子命令 {other}\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(dispatch) {
        Ok(code) => code,
        Err(message) => {
            let _out = writeln!(std::io::stderr(), "xpxe-agent：{message}");
            ExitCode::from(2)
        }
    }
}
