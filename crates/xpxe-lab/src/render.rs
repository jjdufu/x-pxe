//! `render`：用内置预设与节点规格渲染一次装机输入，写到输出目录供实验室 HTTP 端提供。
//!
//! 输出：`boot.ipxe`、`cmdline.txt`、`preseed.cfg`、`disk-select.sh`、`partman-early.sh`、
//! `late.sh`、`xpxe-firstboot.service`，以及空的 `agent`（实验室占位，SHA-256 与渲染时一致）。

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use xpxe_domain::{HttpUrl, NodeSpec, Profile};
use xpxe_render::{
    ArmedState, RenderRequest, RenderRequestParts, RenderedInstall, RequestDigests, RequestTokens,
};

use crate::common::{Args, LabError, LabResult};

/// 默认 HTTP 根。
const DEFAULT_BASE: &str = "http://10.77.0.1:8080";
/// 默认引导令牌。
const DEFAULT_TOKEN: &str = "labboot01";
/// 默认 agent/plan 令牌。
const DEFAULT_AGENT_TOKEN: &str = "labagent01";

/// 文件内容的 SHA-256；文件不存在时按空内容计算。
///
/// # Errors
///
/// 读文件失败（非“不存在”）时返回错误。
fn sha256_of(path: Option<&Path>) -> LabResult<String> {
    let bytes = match path {
        Some(file) => std::fs::read(file)?,
        None => Vec::new(),
    };
    Ok(hex::encode(Sha256::digest(&bytes)))
}

/// 按 `--set /json/pointer=<JSON>` 覆盖预设字段（实验室用来改镜像源等），再重新校验。
///
/// # Errors
///
/// 覆盖语法非法、指针不存在或覆盖后校验失败时返回错误。
fn with_overrides(profile: Profile, sets: &[String]) -> LabResult<Profile> {
    if sets.is_empty() {
        return Ok(profile);
    }
    let mut doc = serde_json::to_value(profile).map_err(|error| LabError(error.to_string()))?;
    for item in sets {
        let (pointer, raw) = item
            .split_once('=')
            .ok_or_else(|| LabError(format!("--set 需要 /指针=JSON：{item}")))?;
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|error| LabError(format!("--set {pointer}：{error}")))?;
        let slot = doc
            .pointer_mut(pointer)
            .ok_or_else(|| LabError(format!("--set 指针不存在：{pointer}")))?;
        *slot = value;
    }
    serde_json::from_value(doc)
        .map_err(|error| LabError(format!("覆盖后 Profile 校验失败：{error}")))
}

/// 组装渲染请求。
///
/// # Errors
///
/// 预设、节点或令牌非法时返回错误。
pub(crate) fn build_request(args: &Args) -> LabResult<RenderRequest> {
    let preset = args.required("--preset")?;
    let base_profile = Profile::load_preset(&preset)
        .map_err(|error| LabError(format!("预设 {preset}：{error}")))?;
    let profile = with_overrides(base_profile, &args.values("--set"))?;
    let node_path = PathBuf::from(args.required("--node")?);
    let node: NodeSpec = serde_json::from_slice(&std::fs::read(&node_path)?)
        .map_err(|error| LabError(format!("节点 {}：{error}", node_path.display())))?;
    let base = args
        .value("--boot-base")
        .unwrap_or_else(|| DEFAULT_BASE.to_owned());
    let token = args
        .value("--token")
        .unwrap_or_else(|| DEFAULT_TOKEN.to_owned());
    let agent_token = args
        .value("--agent-token")
        .unwrap_or_else(|| DEFAULT_AGENT_TOKEN.to_owned());
    let agent = args.value("--agent").map(PathBuf::from);
    let plan = args.value("--plan").map(PathBuf::from);
    let bad = |what: &str| LabError(format!("参数 {what} 非法"));
    Ok(RenderRequest::from_parts(RenderRequestParts {
        profile,
        node,
        boot_base: HttpUrl::parse(&base).map_err(|_parse| bad("--boot-base"))?,
        tokens: RequestTokens {
            boot_token: token.parse().map_err(|_parse| bad("--token"))?,
            agent_token: agent_token.parse().map_err(|_parse| bad("--agent-token"))?,
            session_short: token.chars().take(8).collect(),
            event_id: format!("lab-{token}")
                .parse()
                .map_err(|_parse| bad("--token"))?,
        },
        digests: RequestDigests {
            agent_sha256: sha256_of(agent.as_deref())?,
            plan_sha256: sha256_of(plan.as_deref())?,
        },
        armed: ArmedState::armed(),
    }))
}

/// 输出文件名与内容。
fn outputs(rendered: &RenderedInstall) -> [(&'static str, &str); 7] {
    [
        ("boot.ipxe", rendered.boot_ipxe()),
        ("cmdline.txt", rendered.cmdline()),
        ("preseed.cfg", rendered.preseed()),
        ("disk-select.sh", rendered.disk_select()),
        ("partman-early.sh", rendered.partman_early()),
        ("late.sh", rendered.late()),
        ("xpxe-firstboot.service", rendered.firstboot_unit()),
    ]
}

/// 子命令入口。
///
/// # Errors
///
/// 参数非法、渲染失败或写文件失败时返回错误。
pub(crate) fn run(args: &Args) -> LabResult {
    let out = PathBuf::from(args.required("--out")?);
    let request = build_request(args)?;
    let rendered = xpxe_render::render(&request).map_err(|error| LabError(error.to_string()))?;
    std::fs::create_dir_all(&out)?;
    for (name, body) in outputs(&rendered) {
        std::fs::write(out.join(name), body)?;
    }
    if args.value("--agent").is_none() {
        std::fs::write(out.join("agent"), b"")?;
    }
    if args.value("--plan").is_none() {
        std::fs::write(out.join("plan.json"), b"")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
