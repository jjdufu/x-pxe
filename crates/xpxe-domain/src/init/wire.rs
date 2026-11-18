//! 下发到目标机的计划文件格式（`/var/lib/xpxe/plan.json`），渲染器与 `xpxe-agent` 共用。

use serde::{Deserialize, Serialize};

/// 计划文件 schema 版本。
pub const PLAN_FILE_SCHEMA: u32 = 1;

/// 计划文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanFile {
    /// schema 版本，必须等于 [`PLAN_FILE_SCHEMA`]。
    pub schema: u32,
    /// 装机会话。
    pub session: String,
    /// 事件回报地址（`http://…/p/<agent_token>/events`）。
    pub events_url: String,
    /// 执行日志路径。
    pub log_path: String,
    /// 最终事件（`agent.finished` / `agent.failed`）的持续重试时长（秒）。
    pub final_retry_s: u64,
    /// 按执行顺序排列的全部模块（含被禁用的）。
    pub modules: Vec<PlanEntry>,
    /// 收尾动作。
    pub finalize: PlanFinalize,
}

/// 计划中的一个模块。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanEntry {
    /// 模块 id。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 是否执行；禁用模块记录为 skipped。
    pub enabled: bool,
    /// 超时（秒）。
    pub timeout_s: u64,
    /// 失败后继续。
    pub continue_on_error: bool,
    /// 脚本正文（禁用模块为空串）。
    pub script: String,
    /// 脚本 SHA-256 十六进制。
    pub sha256: String,
}

/// 收尾动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanFinalize {
    /// 成功后重启。
    pub reboot: bool,
}
