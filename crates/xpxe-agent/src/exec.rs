//! 执行单个模块脚本：独立进程组、超时杀死、输出带前缀写入日志与标准输出。

use std::collections::VecDeque;
use std::io::{BufRead as _, BufReader, Read, Write};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// 保留在失败事件里的日志尾行数。
const TAIL_LINES: usize = 50;

/// 模块执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleOutcome {
    /// 退出码；被信号杀死时为 `None`。
    pub exit_code: Option<i32>,
    /// 是否超时被杀。
    pub timed_out: bool,
    /// 最后若干行输出。
    pub tail: Vec<String>,
    /// 耗时。
    pub elapsed: Duration,
}

impl ModuleOutcome {
    /// 是否成功。
    #[must_use]
    pub const fn success(&self) -> bool {
        matches!(self.exit_code, Some(0)) && !self.timed_out
    }
}

/// 把子进程某个输出流逐行转发到通道。
fn forward<S: Read + Send + 'static>(source: S, sink: Sender<String>) {
    let _handle = thread::spawn(move || {
        for line in BufReader::new(source).lines() {
            let Ok(text) = line else { break };
            if sink.send(text).is_err() {
                break;
            }
        }
    });
}

/// 杀死整个进程组（脚本派生的子进程一并结束）。
fn kill_group(child: &mut Child) {
    let group = format!("-{}", child.id());
    let killed = Command::new("kill")
        .args(["-KILL", "--", &group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !killed {
        let _ignored = child.kill();
    }
}

/// 输出收集器：带前缀写日志、回显，并保留尾部。
struct Collector<'log> {
    /// 行前缀（模块 id）。
    prefix: &'log str,
    /// 日志文件。
    log: &'log mut dyn Write,
    /// 尾部缓冲。
    tail: VecDeque<String>,
}

impl Collector<'_> {
    /// 记录一行。
    fn push(&mut self, line: String) {
        let _log = writeln!(self.log, "[{}] {line}", self.prefix);
        let _echo = writeln!(std::io::stdout(), "[{}] {line}", self.prefix);
        if self.tail.len() >= TAIL_LINES {
            let _old = self.tail.pop_front();
        }
        self.tail.push_back(line);
    }

    /// 在截止时间前取通道中的行；通道已关闭时睡到截止时间（避免空转）。
    fn drain(&mut self, lines: &Receiver<String>, until: Instant, idle: bool) {
        loop {
            let wait = until.saturating_duration_since(Instant::now());
            match lines.recv_timeout(wait) {
                Ok(line) => self.push(line),
                Err(RecvTimeoutError::Disconnected) if idle => {
                    thread::sleep(wait);
                    return;
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// 用 `shell` 执行脚本文件，超时后杀死进程组。
///
/// # Errors
///
/// 无法启动或等待子进程时返回错误。
pub fn run_script(
    shell: &Path,
    script: &Path,
    timeout: Duration,
    prefix: &str,
    log: &mut dyn Write,
) -> std::io::Result<ModuleOutcome> {
    let start = Instant::now();
    let mut child = Command::new(shell)
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let (sender, lines) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        forward(out, sender.clone());
    }
    if let Some(err) = child.stderr.take() {
        forward(err, sender);
    }
    let mut collector = Collector {
        prefix,
        log,
        tail: VecDeque::new(),
    };
    let deadline = start.checked_add(timeout).unwrap_or(start);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            kill_group(&mut child);
            break child.wait()?;
        }
        let tick = Instant::now()
            .checked_add(Duration::from_millis(100))
            .unwrap_or(deadline);
        collector.drain(&lines, tick, true);
    };
    let grace = Instant::now()
        .checked_add(Duration::from_secs(2))
        .unwrap_or(deadline);
    collector.drain(&lines, grace, false);
    Ok(ModuleOutcome {
        exit_code: status.code(),
        timed_out,
        tail: collector.tail.into_iter().collect(),
        elapsed: start.elapsed(),
    })
}
