//! 计划执行编排：续跑标记、逐模块执行与回报、成功/失败收尾。

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use xpxe_domain::init::{PlanEntry, PlanFile};

use crate::exec::{ModuleOutcome, run_script};
use crate::host::Host;
use crate::plan::{AgentError, load_plan};
use crate::report::{AgentEvent, Reporter, RetryPolicy, deliver};

/// 运行参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    /// 计划文件。
    pub plan_path: PathBuf,
    /// 状态目录（`done/` 续跑标记、`run/` 脚本落盘）。
    pub state_dir: PathBuf,
    /// 执行模块的 shell。
    pub shell: PathBuf,
    /// 要禁用的 firstboot 单元。
    pub service_unit: String,
    /// 成功后删除的 agent 自身路径。
    pub self_path: Option<PathBuf>,
    /// 覆盖重试策略（测试用）；`None` 时按计划的 `final_retry_s`。
    pub retry: Option<RetryPolicy>,
}

impl AgentConfig {
    /// 生产默认值。
    #[must_use]
    pub fn system(plan_path: PathBuf) -> Self {
        Self {
            plan_path,
            state_dir: PathBuf::from("/var/lib/xpxe"),
            shell: PathBuf::from("/bin/bash"),
            service_unit: "xpxe-firstboot.service".to_owned(),
            self_path: std::env::current_exe().ok(),
            retry: None,
        }
    }
}

/// 运行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// 全部完成并已送达 `agent.finished`。
    Finished {
        /// 是否已发起重启。
        rebooted: bool,
    },
    /// 某模块失败，已送达 `agent.failed`。
    Failed {
        /// 失败模块。
        module: String,
    },
    /// 最终事件在窗口内未送达：不重启，保留计划与服务，下次启动续跑。
    Undelivered {
        /// 未送达的最终事件种类。
        kind: String,
    },
}

/// 一次执行的上下文。
struct Session<'run, H, R> {
    /// 参数。
    config: &'run AgentConfig,
    /// 计划。
    plan: &'run PlanFile,
    /// 宿主。
    host: &'run mut H,
    /// 回报通道。
    reporter: &'run mut R,
    /// 重试策略。
    policy: RetryPolicy,
    /// 日志。
    log: File,
    /// 上一个进度事件未送达：后续进度事件只尝试一次，避免网络中断时逐个模块拖延。
    offline: bool,
}

/// 致命失败。
struct Failure {
    /// 模块 id。
    module: String,
    /// 执行结果。
    outcome: ModuleOutcome,
}

/// 单个模块的结局。
enum Step {
    /// 已完成或跳过。
    Done,
    /// 失败（未设置 `continue_on_error`）。
    Fatal(ModuleOutcome),
}

impl<H: Host, R: Reporter> Session<'_, H, R> {
    /// 写一行 agent 自身日志（同时回显到标准输出供 journal 采集）。
    fn note(&mut self, line: &str) {
        let _log = writeln!(self.log, "[xpxe-agent] {line}");
        let _echo = writeln!(std::io::stdout(), "[xpxe-agent] {line}");
    }

    /// 投递事件；进度事件失败只记日志。
    fn emit(&mut self, kind: &str, module: Option<&str>, payload: Value) -> bool {
        let event = AgentEvent::new(&self.plan.session, kind, module, payload);
        let mut policy = self.policy;
        if self.offline && !event.is_final() {
            policy.progress_attempts = 1;
        }
        let result = deliver(self.host, self.reporter, policy, &event);
        self.offline = result.is_err();
        match result {
            Ok(()) => true,
            Err(error) => {
                self.note(&format!("回报失败：{error}"));
                false
            }
        }
    }

    /// 续跑标记路径。
    fn marker(&self, entry: &PlanEntry) -> PathBuf {
        self.config
            .state_dir
            .join("done")
            .join(format!("{}.{}", entry.id, entry.sha256))
    }

    /// 把脚本写到 `run/<id>.sh`（0700）。
    ///
    /// # Errors
    ///
    /// 写盘失败时返回错误。
    fn stage(&self, entry: &PlanEntry) -> Result<PathBuf, AgentError> {
        let dir = self.config.state_dir.join("run");
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.sh", entry.id));
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o700)
            .open(&path)?;
        file.write_all(entry.script.as_bytes())?;
        Ok(path)
    }

    /// 执行一个模块。
    ///
    /// # Errors
    ///
    /// 落盘或启动 shell 失败时返回错误。
    fn module(&mut self, entry: &PlanEntry) -> Result<Step, AgentError> {
        if !entry.enabled {
            self.note(&format!("--- {} 跳过（未启用）", entry.id));
            return Ok(Step::Done);
        }
        let marker = self.marker(entry);
        if marker.exists() {
            self.note(&format!("--- {} 跳过（已完成，续跑）", entry.id));
            return Ok(Step::Done);
        }
        let script = self.stage(entry)?;
        let _started = self.emit(
            "module.started",
            Some(&entry.id),
            json!({ "title": entry.title }),
        );
        self.note(&format!(
            "==> {} 开始（{}，超时 {}s）",
            entry.id, entry.title, entry.timeout_s
        ));
        let timeout = Duration::from_secs(entry.timeout_s);
        let outcome = run_script(
            &self.config.shell,
            &script,
            timeout,
            &entry.id,
            &mut self.log,
        )?;
        let status = if outcome.success() { "ok" } else { "failed" };
        self.note(&format!(
            "<== {} 结束：{status} exit={:?} timed_out={} {}s",
            entry.id,
            outcome.exit_code,
            outcome.timed_out,
            outcome.elapsed.as_secs()
        ));
        let payload = json!({
            "status": status,
            "exit_code": outcome.exit_code,
            "timed_out": outcome.timed_out,
            "elapsed_s": outcome.elapsed.as_secs(),
        });
        let _finished = self.emit("module.finished", Some(&entry.id), payload);
        if outcome.success() {
            fs::create_dir_all(self.config.state_dir.join("done"))?;
            let _marker = File::create(&marker)?;
            return Ok(Step::Done);
        }
        if entry.continue_on_error {
            self.note(&format!(
                "{} 失败但设置了 continue_on_error，继续",
                entry.id
            ));
            return Ok(Step::Done);
        }
        Ok(Step::Fatal(outcome))
    }

    /// 依次执行全部模块；返回首个致命失败。
    ///
    /// # Errors
    ///
    /// 本地 I/O 失败时返回错误。
    fn modules(&mut self) -> Result<Option<Failure>, AgentError> {
        let plan = self.plan;
        for entry in &plan.modules {
            if let Step::Fatal(outcome) = self.module(entry)? {
                return Ok(Some(Failure {
                    module: entry.id.clone(),
                    outcome,
                }));
            }
        }
        Ok(None)
    }

    /// 禁用 firstboot 服务（失败只记日志）。
    fn disable(&mut self) {
        let unit = self.config.service_unit.clone();
        if let Err(error) = self.host.disable_service(&unit) {
            self.note(&format!("禁用 {unit} 失败：{error}"));
        }
    }

    /// 失败收尾：保留计划与日志，禁用服务。
    fn fail(&mut self, failure: Failure) -> RunOutcome {
        let Failure { module, outcome } = failure;
        let payload = json!({
            "module": module,
            "exit_code": outcome.exit_code,
            "timed_out": outcome.timed_out,
            "log_tail": outcome.tail,
        });
        if !self.emit("agent.failed", None, payload) {
            return RunOutcome::Undelivered {
                kind: "agent.failed".to_owned(),
            };
        }
        self.disable();
        self.note(&format!("初始化失败于 {module}；计划与日志保留"));
        RunOutcome::Failed { module }
    }

    /// 成功收尾：禁用服务、删计划与自身、按计划重启。
    fn finish(&mut self) -> RunOutcome {
        let payload =
            json!({ "modules": self.plan.modules.len(), "reboot": self.plan.finalize.reboot });
        if !self.emit("agent.finished", None, payload) {
            return RunOutcome::Undelivered {
                kind: "agent.finished".to_owned(),
            };
        }
        self.disable();
        remove_quietly(&self.config.plan_path);
        if let Some(path) = self.config.self_path.clone() {
            remove_quietly(&path);
        }
        self.note("初始化完成");
        if !self.plan.finalize.reboot {
            return RunOutcome::Finished { rebooted: false };
        }
        match self.host.reboot() {
            Ok(()) => RunOutcome::Finished { rebooted: true },
            Err(error) => {
                self.note(&format!("重启失败：{error}"));
                RunOutcome::Finished { rebooted: false }
            }
        }
    }
}

/// 删除文件，忽略不存在。
fn remove_quietly(path: &Path) {
    let _removed = fs::remove_file(path);
}

/// 打开（追加）日志文件。
///
/// # Errors
///
/// 无法创建目录或文件时返回错误。
fn open_log(path: &Path) -> Result<File, AgentError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    Ok(OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?)
}

/// 执行计划。
///
/// # Errors
///
/// 非 root、计划无效或本地 I/O 失败时返回错误（此时不回报、不重启）。
pub fn run<H: Host, R: Reporter>(
    config: &AgentConfig,
    host: &mut H,
    reporter: &mut R,
) -> Result<RunOutcome, AgentError> {
    if !host.is_root() {
        return Err(AgentError::new("not_root", "xpxe-agent 必须以 root 运行"));
    }
    let plan = load_plan(&config.plan_path)?;
    let log = open_log(Path::new(&plan.log_path))?;
    let policy = config
        .retry
        .unwrap_or_else(|| RetryPolicy::with_final_window(Duration::from_secs(plan.final_retry_s)));
    let mut session = Session {
        config,
        plan: &plan,
        host,
        reporter,
        policy,
        log,
        offline: false,
    };
    session.note(&format!(
        "会话 {} 开始，共 {} 个模块",
        plan.session,
        plan.modules.len()
    ));
    let enabled = plan.modules.iter().filter(|entry| entry.enabled).count();
    let _sent = session.emit(
        "agent.started",
        None,
        json!({ "modules": plan.modules.len(), "enabled": enabled }),
    );
    Ok(match session.modules()? {
        Some(failure) => session.fail(failure),
        None => session.finish(),
    })
}
