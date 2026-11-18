//! 记录型宿主与回报通道、计划构造工具。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest as _, Sha256};
use xpxe_agent::{AgentConfig, AgentEvent, Host, Reporter, RetryPolicy};
use xpxe_domain::init::{PLAN_FILE_SCHEMA, PlanEntry, PlanFile, PlanFinalize};
use xpxe_test_support::TempDataDir;

/// 记录型宿主：睡眠推进虚拟时钟。
#[derive(Debug)]
pub(crate) struct FakeHost {
    /// 是否 root。
    pub(crate) root: bool,
    /// 被禁用的单元。
    pub(crate) disabled: Vec<String>,
    /// 重启次数。
    pub(crate) reboots: u32,
    /// 累计睡眠。
    pub(crate) slept: Duration,
    /// 时钟起点。
    pub(crate) base: Instant,
}

impl FakeHost {
    /// root 宿主。
    pub(crate) fn root() -> Self {
        Self {
            root: true,
            disabled: Vec::new(),
            reboots: 0,
            slept: Duration::ZERO,
            base: Instant::now(),
        }
    }
}

impl Host for FakeHost {
    fn is_root(&self) -> bool {
        self.root
    }

    fn disable_service(&mut self, unit: &str) -> Result<(), String> {
        self.disabled.push(unit.to_owned());
        Ok(())
    }

    fn reboot(&mut self) -> Result<(), String> {
        self.reboots = self.reboots.saturating_add(1);
        Ok(())
    }

    fn sleep(&mut self, duration: Duration) {
        self.slept = self.slept.saturating_add(duration);
    }

    fn now(&self) -> Instant {
        self.base.checked_add(self.slept).unwrap_or(self.base)
    }
}

/// 记录型回报：前 `fail_first` 次失败，`down` 时一直失败。
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    /// 已送达事件。
    pub(crate) events: Vec<AgentEvent>,
    /// 尝试次数。
    pub(crate) attempts: u32,
    /// 前若干次失败。
    pub(crate) fail_first: u32,
    /// 一直失败。
    pub(crate) down: bool,
}

impl Recorder {
    /// 已送达事件的 `kind[:module]` 序列。
    pub(crate) fn trail(&self) -> Vec<String> {
        self.events
            .iter()
            .map(|event| event.event_id.clone())
            .collect()
    }
}

impl Reporter for Recorder {
    fn send(&mut self, event: &AgentEvent) -> Result<(), String> {
        self.attempts = self.attempts.saturating_add(1);
        if self.down || self.attempts <= self.fail_first {
            return Err("模拟网络故障".to_owned());
        }
        self.events.push(event.clone());
        Ok(())
    }
}

/// 一个模块条目。
pub(crate) fn entry(id: &str, script: &str, timeout_s: u64) -> PlanEntry {
    PlanEntry {
        id: id.to_owned(),
        title: format!("模块 {id}"),
        enabled: true,
        timeout_s,
        continue_on_error: false,
        script: script.to_owned(),
        sha256: hex::encode(Sha256::digest(script.as_bytes())),
    }
}

/// 被禁用的条目。
pub(crate) fn disabled(id: &str) -> PlanEntry {
    PlanEntry {
        enabled: false,
        ..entry(id, "", 60)
    }
}

/// 测试工作区：临时目录、计划、配置。
pub(crate) struct Workspace {
    /// 临时目录（析构删除）。
    pub(crate) dir: TempDataDir,
    /// 配置。
    pub(crate) config: AgentConfig,
}

impl Workspace {
    /// 写计划并生成配置。
    ///
    /// # Errors
    ///
    /// 写盘失败时返回错误。
    pub(crate) fn new(modules: Vec<PlanEntry>, reboot: bool) -> std::io::Result<Self> {
        let dir = TempDataDir::new()?;
        let plan = PlanFile {
            schema: PLAN_FILE_SCHEMA,
            session: "sess01".to_owned(),
            events_url: "http://127.0.0.1:9/p/agent001/events".to_owned(),
            log_path: dir.path().join("log/firstboot.log").display().to_string(),
            final_retry_s: 60,
            modules,
            finalize: PlanFinalize { reboot },
        };
        let plan_path = dir.path().join("plan.json");
        fs::write(
            &plan_path,
            serde_json::to_vec(&plan).map_err(std::io::Error::other)?,
        )?;
        let self_path = dir.path().join("xpxe-agent");
        fs::write(&self_path, b"binary")?;
        let config = AgentConfig {
            plan_path,
            state_dir: dir.path().join("state"),
            shell: PathBuf::from("/bin/sh"),
            service_unit: "xpxe-firstboot.service".to_owned(),
            self_path: Some(self_path),
            retry: Some(RetryPolicy::with_final_window(Duration::from_secs(60))),
        };
        Ok(Self { dir, config })
    }

    /// 临时目录下的路径。
    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// 日志内容。
    ///
    /// # Errors
    ///
    /// 读失败时返回错误。
    pub(crate) fn log(&self) -> std::io::Result<String> {
        fs::read_to_string(self.path("log/firstboot.log"))
    }
}

/// 路径是否存在。
pub(crate) fn exists(path: &Path) -> bool {
    path.exists()
}
