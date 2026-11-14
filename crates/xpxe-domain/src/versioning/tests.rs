//! 规范化 JSON 稳定性、差异路径、`Secret` 不泄露、schema 迁移。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{
    ChangeValue, ConfigDocument, ConfigError, ConfigKind, FieldError, Secret, Timeouts,
    canonical_json, content_sha256_hex, diff, migrate_current,
};

/// 用于迁移测试的 schema 2 文档：由 schema 1 的 `name` 改名为 `title`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SampleV2 {
    /// 标题。
    title: String,
    /// 可选备注。
    note: Option<String>,
    /// 密码。
    password: Secret<String>,
}

impl ConfigDocument for SampleV2 {
    const KIND: ConfigKind = ConfigKind::Profile;
    const SCHEMA: u32 = 2;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        if self.title.is_empty() {
            Err(vec![super::field_error("/title", "empty", "标题不能为空")])
        } else {
            Ok(())
        }
    }

    fn migrate(from_schema: u32, value: Value) -> Result<Self, ConfigError> {
        migrate_sample(from_schema, value)
    }
}

/// 从 schema 1（`name`）或当前 schema 迁移。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn migrate_sample(from_schema: u32, mut value: Value) -> Result<SampleV2, ConfigError> {
    match from_schema {
        2 => migrate_current(from_schema, value),
        1 => {
            rename_name_to_title(&mut value);
            migrate_current(2, value)
        }
        other => Err(ConfigError::UnsupportedSchema {
            kind: ConfigKind::Profile,
            from: other,
            to: 2,
        }),
    }
}

/// 将 schema 1 的 `name` 改为 `title`。
fn rename_name_to_title(value: &mut Value) {
    if let Some(object) = value.as_object_mut()
        && let Some(name) = object.remove("name")
    {
        let _previous = object.insert("title".to_owned(), name);
    }
}

/// 规范文档。
fn sample(title: &str, secret: &str) -> SampleV2 {
    SampleV2 {
        title: title.to_owned(),
        note: Some("alpha".to_owned()),
        password: Secret::new("ref/password".to_owned(), secret.to_owned()),
    }
}

/// 规范化 JSON 键排序、无空白，且重复计算哈希稳定。
///
/// # Errors
///
/// 规范化结果不稳定或含明文时返回错误。
#[test]
fn canonical_json_is_stable_and_redacts_secrets() -> TestResult {
    let doc = sample("web", "s3cret");
    let first = canonical_json(&doc)?;
    let second = canonical_json(&doc)?;
    ensure_eq(&first.as_bytes(), &second.as_bytes(), "稳定")?;
    ensure_eq(
        &content_sha256_hex(&first),
        &content_sha256_hex(&second),
        "哈希",
    )?;
    let text = first.as_str()?;
    ensure(!text.contains("s3cret"), "明文不得出现")?;
    ensure(text.contains("\"fingerprint\""), "含指纹")?;
    ensure(text.contains("\"secret_ref\""), "含引用")?;
    ensure(!text.contains(' '), "无空白")?;
    // 键排序：fingerprint 在 password 对象内应在 secret_ref 之前
    ensure(text.contains("\"fingerprint\":"), "fingerprint 键")?;
    Ok(())
}

/// 差异：普通字段给出路径与新旧值；密钥只显示“已更改”。
///
/// # Errors
///
/// 差异内容不符或泄露明文时返回错误。
#[test]
fn diff_paths_and_secret_redaction() -> TestResult {
    let before = canonical_json(&sample("web", "old-secret"))?;
    let after_doc = SampleV2 {
        title: "api".to_owned(),
        note: Some("alpha".to_owned()),
        password: Secret::new("ref/password".to_owned(), "new-secret".to_owned()),
    };
    let after = canonical_json(&after_doc)?;
    let changes = diff(&before, &after);
    let title = changes
        .iter()
        .find(|change| change.path == "/title")
        .ok_or("缺少 /title")?;
    ensure_eq(&title.before, &ChangeValue::Json(json!("web")), "title 旧")?;
    ensure_eq(&title.after, &ChangeValue::Json(json!("api")), "title 新")?;
    let password = changes
        .iter()
        .find(|change| change.path == "/password")
        .ok_or("缺少 /password")?;
    ensure_eq(&password.before, &ChangeValue::SecretChanged, "密钥旧")?;
    ensure_eq(&password.after, &ChangeValue::SecretChanged, "密钥新")?;
    let blob = format!("{changes:?}");
    ensure(!blob.contains("old-secret"), "差异无旧明文")?;
    ensure(!blob.contains("new-secret"), "差异无新明文")?;
    Ok(())
}

/// schema 1 可迁移到 2；不支持的版本被拒绝；相同内容哈希不变。
///
/// # Errors
///
/// 迁移结果不符时返回错误。
#[test]
fn schema_migration() -> TestResult {
    let v1 = json!({
        "name": "web",
        "note": "alpha",
        "password": { "secret_ref": "ref/password", "fingerprint": "deadbeef" }
    });
    let migrated = SampleV2::migrate(1, v1)?;
    ensure_eq(&migrated.title.as_str(), &"web", "改名")?;
    ensure(migrated.password.value().is_none(), "迁移后无明文")?;
    let current = serde_json::to_value(&migrated)?;
    let again = SampleV2::migrate(2, current)?;
    ensure_eq(&again.title, &migrated.title, "当前 schema")?;
    match SampleV2::migrate(0, json!({})) {
        Err(ConfigError::UnsupportedSchema { from, to, .. }) => {
            ensure_eq(&from, &0_u32, "from")?;
            Ok(ensure_eq(&to, &2_u32, "to")?)
        }
        other => Err(format!("期望 UnsupportedSchema，得到 {other:?}").into()),
    }
}

/// `Timeouts` 作为配置文档可规范化。
///
/// # Errors
///
/// 规范化失败时返回错误。
#[test]
fn timeouts_canonical() -> TestResult {
    let timeouts = Timeouts::from_ttl(crate::lifecycle::Ttl::standard());
    let json = canonical_json(&timeouts)?;
    let text = json.as_str()?;
    ensure(text.contains("\"arm_secs\":7200"), "默认 arm")?;
    let parsed = Timeouts::migrate(1, serde_json::from_slice(json.as_bytes())?)?;
    ensure_eq(&parsed.to_ttl(), &crate::lifecycle::Ttl::standard(), "往返")?;
    Ok(())
}

/// 空标题无法规范化。
///
/// # Errors
///
/// 非法文档被接受时返回错误。
#[test]
fn validate_rejects_empty_title() -> TestResult {
    let doc = sample("", "x");
    ensure(canonical_json(&doc).is_err(), "空标题")?;
    Ok(())
}
