//! 读取并校验计划文件。

use std::collections::BTreeSet;
use std::path::Path;

use sha2::{Digest as _, Sha256};
use xpxe_domain::init::{PLAN_FILE_SCHEMA, PlanFile};

/// agent 错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentError {
    /// 稳定错误码。
    pub code: &'static str,
    /// 说明。
    pub message: String,
}

impl AgentError {
    /// 构造错误。
    #[must_use]
    pub fn new<M: Into<String>>(code: &'static str, message: M) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AgentError {}

impl From<std::io::Error> for AgentError {
    fn from(error: std::io::Error) -> Self {
        Self::new("io", error.to_string())
    }
}

/// 读取计划文件并校验。
///
/// # Errors
///
/// 读文件、JSON 解析或校验失败时返回错误。
pub fn load_plan(path: &Path) -> Result<PlanFile, AgentError> {
    let raw = std::fs::read(path)?;
    let plan: PlanFile = serde_json::from_slice(&raw)
        .map_err(|error| AgentError::new("plan_json", error.to_string()))?;
    verify_plan(&plan)?;
    Ok(plan)
}

/// 校验 schema、模块 id、事件地址与每个模块的 SHA-256。
///
/// # Errors
///
/// 任一检查失败时返回错误。
pub fn verify_plan(plan: &PlanFile) -> Result<(), AgentError> {
    if plan.schema != PLAN_FILE_SCHEMA {
        return Err(AgentError::new(
            "plan_schema",
            format!(
                "计划 schema {}，agent 只支持 {PLAN_FILE_SCHEMA}",
                plan.schema
            ),
        ));
    }
    if !plan.events_url.starts_with("http://") {
        return Err(AgentError::new(
            "plan_events_url",
            "事件地址必须是 http://（装机监听）",
        ));
    }
    let mut seen = BTreeSet::new();
    for entry in &plan.modules {
        let id_ok = !entry.id.is_empty()
            && entry
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        if !id_ok || !seen.insert(entry.id.as_str()) {
            return Err(AgentError::new(
                "plan_module_id",
                format!("模块 id 非法或重复：{}", entry.id),
            ));
        }
        let actual = hex::encode(Sha256::digest(entry.script.as_bytes()));
        if actual != entry.sha256 {
            return Err(AgentError::new(
                "plan_sha256",
                format!(
                    "模块 {} 的 SHA-256 不符（计划 {}，实际 {actual}）",
                    entry.id, entry.sha256
                ),
            ));
        }
        if entry.enabled && entry.script.is_empty() {
            return Err(AgentError::new(
                "plan_empty_script",
                format!("启用的模块 {} 没有脚本", entry.id),
            ));
        }
    }
    Ok(())
}
