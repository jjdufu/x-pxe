//! `node-check --spec <json>`：按节点登记的跨字段约束校验节点规格（Step 9 `redfish-bios-reject`）。
//!
//! 输出 `{"event":"node_check","ok":true}` 或带字段级错误（`path`/`code`/`message`）的 `ok:false`；
//! 规格被拒绝不算命令失败。

use serde_json::json;
use xpxe_domain::{NodeSpec, NodeSpecParts};

use crate::common::{Args, LabResult, emit};

/// 子命令入口。
///
/// # Errors
///
/// 文件不可读或 JSON 结构不合法时返回错误。
pub(crate) fn run(args: &Args) -> LabResult {
    let raw = std::fs::read(args.required("--spec")?)?;
    let parts: NodeSpecParts = serde_json::from_slice(&raw)?;
    match NodeSpec::new(parts) {
        Ok(_spec) => emit(&json!({"event": "node_check", "ok": true})),
        Err(errors) => emit(&json!({
            "event": "node_check",
            "ok": false,
            "errors": errors
                .iter()
                .map(|error| json!({"path": error.path, "code": error.code, "message": error.message}))
                .collect::<Vec<_>>(),
        })),
    }
}
