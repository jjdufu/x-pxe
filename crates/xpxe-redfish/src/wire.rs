//! Redfish JSON 辅助：`@odata.id` 链接、集合成员、动作目标、`ExtendedInfo` 与请求体。

use serde_json::{Map, Value, json};

use crate::profile::{InsertFields, TransferFields};

/// `resource[key]["@odata.id"]`。
pub(crate) fn link(resource: &Value, key: &str) -> Option<String> {
    resource
        .get(key)?
        .get("@odata.id")?
        .as_str()
        .map(ToOwned::to_owned)
}

/// 集合的 `Members[*]["@odata.id"]`。
pub(crate) fn members(collection: &Value) -> Vec<String> {
    collection
        .get("Members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("@odata.id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

/// 字符串字段。
pub(crate) fn text<'value>(resource: &'value Value, key: &str) -> Option<&'value str> {
    resource.get(key).and_then(Value::as_str)
}

/// 动作目标：优先 `Actions["#<name>"].target`，否则 `<resource>/Actions/<name>`。
pub(crate) fn action_target(resource: &Value, path: &str, name: &str) -> String {
    resource
        .get("Actions")
        .and_then(|actions| actions.get(format!("#{name}")))
        .and_then(|action| action.get("target"))
        .and_then(Value::as_str)
        .map_or_else(
            || format!("{}/Actions/{name}", path.trim_end_matches('/')),
            ToOwned::to_owned,
        )
}

/// 介质类型是否含 CD/DVD。
pub(crate) fn is_cd(media: &Value) -> bool {
    media
        .get("MediaTypes")
        .and_then(Value::as_array)
        .is_some_and(|types| {
            types
                .iter()
                .filter_map(Value::as_str)
                .any(|kind| kind == "CD" || kind == "DVD")
        })
}

/// `Boot.BootSourceOverrideTarget@Redfish.AllowableValues` 是否允许 `Cd`（未列出时视为允许）。
pub(crate) fn cd_allowed(system: &Value) -> bool {
    system
        .get("Boot")
        .and_then(|boot| boot.get("BootSourceOverrideTarget@Redfish.AllowableValues"))
        .and_then(Value::as_array)
        .is_none_or(|values| values.iter().any(|value| value.as_str() == Some("Cd")))
}

/// 从错误响应中提取说明：`error.message` 与各级 `@Message.ExtendedInfo[*].Message`。
pub(crate) fn extended_info(body: &Value) -> String {
    let mut out: Vec<String> = Vec::new();
    let error = body.get("error").unwrap_or(body);
    if let Some(message) = text(error, "message") {
        out.push(message.to_owned());
    }
    for holder in [error, body] {
        let infos = holder
            .get("@Message.ExtendedInfo")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for info in infos {
            let message = text(info, "Message").unwrap_or_default();
            let id = text(info, "MessageId").unwrap_or_default();
            let line = if id.is_empty() {
                message.to_owned()
            } else {
                format!("{message}（{id}）")
            };
            if !line.is_empty() && !out.contains(&line) {
                out.push(line);
            }
        }
    }
    out.join("；")
}

/// `InsertMedia` 请求体（按画像裁剪字段）。
pub(crate) fn insert_body(image: &str, fields: InsertFields) -> Value {
    let mut body = Map::new();
    let _image = body.insert("Image".to_owned(), json!(image));
    let _inserted = body.insert("Inserted".to_owned(), json!(true));
    if fields.write_protected {
        let _protected = body.insert("WriteProtected".to_owned(), json!(true));
    }
    if fields.transfer == TransferFields::MethodAndProtocol {
        let _method = body.insert("TransferMethod".to_owned(), json!("Stream"));
    }
    if fields.transfer != TransferFields::Omit {
        let _protocol = body.insert("TransferProtocolType".to_owned(), json!("HTTP"));
    }
    Value::Object(body)
}

/// 一次性 CD 启动的 `PATCH` 体。
pub(crate) fn boot_body(with_mode: bool) -> Value {
    let mut boot = Map::new();
    let _target = boot.insert("BootSourceOverrideTarget".to_owned(), json!("Cd"));
    let _enabled = boot.insert("BootSourceOverrideEnabled".to_owned(), json!("Once"));
    if with_mode {
        let _mode = boot.insert("BootSourceOverrideMode".to_owned(), json!("UEFI"));
    }
    json!({ "Boot": Value::Object(boot) })
}

#[cfg(test)]
mod tests;
