//! `render-plan`：用内置初始化预设渲染一份计划文件（Step 5），可选写出逐模块脚本。
//!
//! 参数：`--preset <deb13-init-scripts|minimal>`、`--out <plan.json>`、`[--profile official-mirror]`、
//! `[--set /json/pointer=<JSON>]…`、`[--scripts <dir>]`、`[--session lab-init]`、
//! `[--events-url http://10.77.0.1:8080/p/labagent01/events]`、`[--final-retry 600]`。

use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;
use xpxe_domain::init::{PlainPath, TargetPlan};
use xpxe_domain::{HttpUrl, Profile, SessionId};
use xpxe_render::init::{DEFAULT_LOG_PATH, PlanContext, render_plan};

use crate::common::{Args, LabError, LabResult};

/// 按 `--set /指针=JSON` 覆盖任意可序列化文档后重新解析。
///
/// # Errors
///
/// 覆盖语法非法、指针不存在或重新解析失败时返回错误。
pub(crate) fn apply_sets<T: Serialize + DeserializeOwned>(doc: T, sets: &[String]) -> LabResult<T> {
    let mut value = serde_json::to_value(doc).map_err(|error| LabError(error.to_string()))?;
    for item in sets {
        let (pointer, raw) = item
            .split_once('=')
            .ok_or_else(|| LabError(format!("--set 需要 /指针=JSON：{item}")))?;
        let replacement: serde_json::Value = serde_json::from_str(raw)
            .map_err(|error| LabError(format!("--set {pointer}：{error}")))?;
        let slot = value
            .pointer_mut(pointer)
            .ok_or_else(|| LabError(format!("--set 指针不存在：{pointer}")))?;
        *slot = replacement;
    }
    serde_json::from_value(value).map_err(|error| LabError(format!("覆盖后校验失败：{error}")))
}

/// 子命令入口。
///
/// # Errors
///
/// 参数、预设、渲染或写文件失败时返回错误。
pub(crate) fn run(args: &Args) -> LabResult {
    let preset = args.required("--preset")?;
    let base_plan = TargetPlan::load_preset(&preset)
        .map_err(|error| LabError(format!("初始化预设 {preset}：{error}")))?;
    let plan = apply_sets(base_plan, &args.values("--set"))?;
    let profile_name = args
        .value("--profile")
        .unwrap_or_else(|| "official-mirror".to_owned());
    let profile = Profile::load_preset(&profile_name)
        .map_err(|error| LabError(format!("Profile {profile_name}：{error}")))?;
    let invalid = |error: xpxe_domain::ParseError| LabError(error.to_string());
    let ctx = PlanContext {
        session: SessionId::parse(
            &args
                .value("--session")
                .unwrap_or_else(|| "lab-init".to_owned()),
        )
        .map_err(invalid)?,
        events_url: HttpUrl::parse(
            &args
                .value("--events-url")
                .unwrap_or_else(|| "http://10.77.0.1:8080/p/labagent01/events".to_owned()),
        )
        .map_err(invalid)?,
        log_path: PlainPath::parse(DEFAULT_LOG_PATH).map_err(invalid)?,
        final_retry_s: args.parsed("--final-retry", 600_u64)?,
    };
    let rendered =
        render_plan(&plan, &profile, &ctx).map_err(|error| LabError(error.to_string()))?;
    let out = PathBuf::from(args.required("--out")?);
    std::fs::write(&out, rendered.json.as_bytes())?;
    if let Some(dir) = args.value("--scripts") {
        std::fs::create_dir_all(&dir)?;
        for module in &rendered.modules {
            std::fs::write(
                PathBuf::from(&dir).join(format!("{}.sh", module.id.as_str())),
                &module.script,
            )?;
        }
    }
    let warnings: Vec<String> = rendered.warnings.iter().map(ToString::to_string).collect();
    crate::common::emit(&serde_json::json!({
        "plan": out.display().to_string(),
        "modules": rendered.file.modules.len(),
        "enabled": rendered.file.modules.iter().filter(|entry| entry.enabled).count(),
        "warnings": warnings,
    }))
}
