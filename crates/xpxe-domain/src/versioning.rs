//! 配置文档、规范化 JSON、结构化差异与 `Secret<T>`。

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::lifecycle::Ttl;

/// 配置对象种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigKind {
    /// 安装配置档案。
    Profile,
    /// 初始化计划。
    InitPlan,
    /// 引导网络。
    BootNetwork,
    /// DHCP 应答规则。
    DhcpRules,
    /// 状态机超时。
    Timeouts,
    /// 资产频道。
    AssetChannels,
    /// 节点规格。
    NodeSpec,
    /// BMC。
    Bmc,
}

impl ConfigKind {
    /// 稳定的蛇形名称。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::InitPlan => "init_plan",
            Self::BootNetwork => "boot_network",
            Self::DhcpRules => "dhcp_rules",
            Self::Timeouts => "timeouts",
            Self::AssetChannels => "asset_channels",
            Self::NodeSpec => "node_spec",
            Self::Bmc => "bmc",
        }
    }
}

impl Display for ConfigKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 字段级校验错误，`path` 使用 JSON Pointer。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// JSON Pointer，例如 `/channel`。
    pub path: String,
    /// 稳定错误码，例如 `redfish_requires_uefi`。
    pub code: String,
    /// 人类可读说明。
    pub message: String,
}

impl Display for FieldError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} ({})", self.path, self.code, self.message)
    }
}

/// 构造字段错误。
#[must_use]
pub fn field_error(path: &str, code: &str, message: &str) -> FieldError {
    FieldError {
        path: path.to_owned(),
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

/// 一组字段错误，供 serde `try_from` 使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// 字段错误列表。
    errors: Vec<FieldError>,
}

impl ValidationError {
    /// 由字段错误列表构造。
    #[must_use]
    pub const fn from_fields(errors: Vec<FieldError>) -> Self {
        Self { errors }
    }

    /// 字段错误。
    #[must_use]
    pub fn errors(&self) -> &[FieldError] {
        &self.errors
    }
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for error in &self.errors {
            if !first {
                f.write_str("; ")?;
            }
            first = false;
            Display::fmt(error, f)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationError {}

/// 配置处理失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// 文档未通过校验。
    Invalid(Box<ValidationError>),
    /// 没有从 `from` 升到当前 `SCHEMA` 的迁移。
    UnsupportedSchema {
        /// 文档种类。
        kind: ConfigKind,
        /// 来源版本。
        from: u32,
        /// 目标版本。
        to: u32,
    },
    /// JSON 编解码失败。
    Json(String),
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(error) => write!(f, "校验失败：{error}"),
            Self::UnsupportedSchema { kind, from, to } => {
                write!(f, "{kind} 无法从 schema {from} 迁移到 {to}")
            }
            Self::Json(error) => write!(f, "JSON 错误：{error}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<serde_json::Error> for ConfigError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value.to_string())
    }
}

/// 可版本化的配置文档。
pub trait ConfigDocument: Serialize + DeserializeOwned + Sized {
    /// 对象种类。
    const KIND: ConfigKind;
    /// 当前 schema 版本。
    const SCHEMA: u32;

    /// 字段级校验。
    ///
    /// # Errors
    ///
    /// 存在非法字段时返回全部 [`FieldError`]。
    fn validate(&self) -> Result<(), Vec<FieldError>>;

    /// 从旧 schema 的 JSON 迁移到当前类型。
    ///
    /// # Errors
    ///
    /// 不支持的版本、JSON 形状不符或校验失败时返回错误。
    fn migrate(from_schema: u32, value: Value) -> Result<Self, ConfigError>;
}

/// 当 `from_schema` 已是当前版本时，反序列化并校验。
///
/// # Errors
///
/// 版本不符、解码失败或校验失败时返回错误。
pub fn migrate_current<T: ConfigDocument>(
    from_schema: u32,
    value: Value,
) -> Result<T, ConfigError> {
    if from_schema != T::SCHEMA {
        return Err(ConfigError::UnsupportedSchema {
            kind: T::KIND,
            from: from_schema,
            to: T::SCHEMA,
        });
    }
    let doc: T = serde_json::from_value(value)?;
    doc.validate()
        .map_err(|errors| ConfigError::Invalid(Box::new(ValidationError::from_fields(errors))))?;
    Ok(doc)
}

/// 规范化 JSON 字节（键排序、无多余空白、数字统一格式）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalJson {
    /// UTF-8 JSON 字节。
    bytes: Vec<u8>,
}

impl CanonicalJson {
    /// 规范化字节。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// UTF-8 文本。
    ///
    /// # Errors
    ///
    /// 字节不是合法 UTF-8 时返回错误。
    pub fn as_str(&self) -> Result<&str, ConfigError> {
        std::str::from_utf8(&self.bytes).map_err(|error| ConfigError::Json(error.to_string()))
    }
}

/// 将文档校验后写成规范化 JSON。
///
/// # Errors
///
/// 校验失败或序列化失败时返回错误。
pub fn canonical_json<T: ConfigDocument>(doc: &T) -> Result<CanonicalJson, ConfigError> {
    doc.validate()
        .map_err(|errors| ConfigError::Invalid(Box::new(ValidationError::from_fields(errors))))?;
    let value = serde_json::to_value(doc)?;
    let mut bytes = Vec::new();
    write_canonical(&value, &mut bytes)?;
    Ok(CanonicalJson { bytes })
}

/// 规范化内容的 SHA-256。相同内容重复保存不得产生新版本。
///
/// # Errors
///
/// 失败时返回对应错误。
#[must_use]
pub fn content_sha256(json: &CanonicalJson) -> [u8; 32] {
    let digest = Sha256::digest(json.as_bytes());
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// 以十六进制表示内容哈希。
#[must_use]
pub fn content_sha256_hex(json: &CanonicalJson) -> String {
    hex::encode(content_sha256(json))
}

/// 将 `serde_json::Value` 写成规范化形式。
///
/// # Errors
///
/// 失败时返回对应错误。
fn write_canonical(value: &Value, out: &mut Vec<u8>) -> Result<(), ConfigError> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(number) => write_number(number, out),
        Value::String(text) => write_string(text, out),
        Value::Array(items) => write_array(items, out)?,
        Value::Object(map) => write_object(map, out)?,
    }
    Ok(())
}

/// 统一数字格式：优先无符号，再有符号，否则用 JSON 库的原文。
///
/// # Errors
///
/// 失败时返回对应错误。
fn write_number(number: &serde_json::Number, out: &mut Vec<u8>) {
    if let Some(value) = number.as_u64() {
        out.extend_from_slice(value.to_string().as_bytes());
    } else if let Some(value) = number.as_i64() {
        out.extend_from_slice(value.to_string().as_bytes());
    } else {
        out.extend_from_slice(number.to_string().as_bytes());
    }
}

/// 按 JSON 规则转义字符串。
/// 按 JSON 规则转义字符串。
fn write_string(text: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for item in text.chars() {
        match item {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\u{0C}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            _ if u32::from(item) < 0x20_u32 => push_unicode_escape(out, item),
            _ => push_char(out, item),
        }
    }
    out.push(b'"');
}

/// 写入一个非转义字符的 UTF-8 字节。
fn push_char(out: &mut Vec<u8>, ch: char) {
    let mut buf = [0_u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    out.extend_from_slice(encoded.as_bytes());
}

/// 写入 `\u00XX`。
fn push_unicode_escape(out: &mut Vec<u8>, ch: char) {
    let code = u32::from(ch);
    let hex = format!("{code:04x}");
    out.extend_from_slice(br"\u");
    out.extend_from_slice(hex.as_bytes());
}

/// 写入数组。
///
/// # Errors
///
/// 失败时返回对应错误。
fn write_array(items: &[Value], out: &mut Vec<u8>) -> Result<(), ConfigError> {
    out.push(b'[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        write_canonical(item, out)?;
    }
    out.push(b']');
    Ok(())
}

/// 写入键排序后的对象。
///
/// # Errors
///
/// 失败时返回对应错误。
fn write_object(map: &Map<String, Value>, out: &mut Vec<u8>) -> Result<(), ConfigError> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    out.push(b'{');
    for (index, (key, value)) in entries.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        write_string(key, out);
        out.push(b':');
        write_canonical(value, out)?;
    }
    out.push(b'}');
    Ok(())
}

/// 差异中的值：普通 JSON 或已改动的密钥（永不包含明文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeValue {
    /// 普通 JSON 值。
    Json(Value),
    /// 密钥字段已更改，展示为“已更改”。
    SecretChanged,
}

impl Display for ChangeValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(value) => write!(f, "{value}"),
            Self::SecretChanged => f.write_str("已更改"),
        }
    }
}

/// 一处结构化差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// JSON Pointer。
    pub path: String,
    /// 变更前。
    pub before: ChangeValue,
    /// 变更后。
    pub after: ChangeValue,
}

/// 比较两份规范化 JSON，密钥字段只显示“已更改”。
///
/// # Errors
///
/// 失败时返回对应错误。
#[must_use]
pub fn diff(left: &CanonicalJson, right: &CanonicalJson) -> Vec<Change> {
    let Ok(left_value) = serde_json::from_slice::<Value>(left.as_bytes()) else {
        return Vec::new();
    };
    let Ok(right_value) = serde_json::from_slice::<Value>(right.as_bytes()) else {
        return Vec::new();
    };
    let mut changes = Vec::new();
    walk_diff(&mut changes, "", &left_value, &right_value);
    changes
}

/// 递归比较，路径使用 JSON Pointer。
fn walk_diff(out: &mut Vec<Change>, path: &str, left: &Value, right: &Value) {
    if left == right {
        return;
    }
    if is_secret(left) || is_secret(right) {
        out.push(Change {
            path: path.to_owned(),
            before: ChangeValue::SecretChanged,
            after: ChangeValue::SecretChanged,
        });
        return;
    }
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            walk_objects(out, path, left_map, right_map);
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            walk_arrays(out, path, left_items, right_items);
        }
        (_, _) => out.push(Change {
            path: path.to_owned(),
            before: ChangeValue::Json(left.clone()),
            after: ChangeValue::Json(right.clone()),
        }),
    }
}

/// 比较两个对象。
fn walk_objects(
    out: &mut Vec<Change>,
    path: &str,
    left: &Map<String, Value>,
    right: &Map<String, Value>,
) {
    let mut keys = BTreeSet::<&str>::new();
    for key in left.keys().chain(right.keys()) {
        let _inserted = keys.insert(key.as_str());
    }
    for key in &keys {
        let child = json_pointer(path, key);
        match (left.get(*key), right.get(*key)) {
            (Some(left_value), Some(right_value)) => {
                walk_diff(out, &child, left_value, right_value);
            }
            (Some(left_value), None) => out.push(Change {
                path: child,
                before: change_of(left_value),
                after: ChangeValue::Json(Value::Null),
            }),
            (None, Some(right_value)) => out.push(Change {
                path: child,
                before: ChangeValue::Json(Value::Null),
                after: change_of(right_value),
            }),
            (None, None) => {}
        }
    }
}

/// 比较两个数组（按索引对齐）。
fn walk_arrays(out: &mut Vec<Change>, path: &str, left: &[Value], right: &[Value]) {
    let len = left.len().max(right.len());
    for index in 0..len {
        let child = format!("{path}/{index}");
        match (left.get(index), right.get(index)) {
            (Some(left_value), Some(right_value)) => {
                walk_diff(out, &child, left_value, right_value);
            }
            (Some(left_value), None) => out.push(Change {
                path: child,
                before: change_of(left_value),
                after: ChangeValue::Json(Value::Null),
            }),
            (None, Some(right_value)) => out.push(Change {
                path: child,
                before: ChangeValue::Json(Value::Null),
                after: change_of(right_value),
            }),
            (None, None) => {}
        }
    }
}

/// 将值转为差异值，密钥对象一律打码。
fn change_of(value: &Value) -> ChangeValue {
    if is_secret(value) {
        ChangeValue::SecretChanged
    } else {
        ChangeValue::Json(value.clone())
    }
}

/// 是否为 `Secret` 的序列化形状。
fn is_secret(value: &Value) -> bool {
    value.as_object().is_some_and(is_secret_map)
}

/// `Secret` 或带 `hash` 的口令哈希包装。
fn is_secret_map(map: &Map<String, Value>) -> bool {
    if !map.contains_key("secret_ref") || !map.contains_key("fingerprint") {
        return false;
    }
    match map.len() {
        2 => true,
        3 => map.contains_key("hash"),
        _ => false,
    }
}

/// 追加一个 JSON Pointer 段。
fn json_pointer(parent: &str, key: &str) -> String {
    let escaped = key.replace('~', "~0").replace('/', "~1");
    if parent.is_empty() {
        format!("/{escaped}")
    } else {
        format!("{parent}/{escaped}")
    }
}

/// 包装敏感值。序列化为引用与指纹，差异中只显示“已更改”。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secret<T> {
    /// 密钥引用（控制面/存储使用）。
    key_ref: String,
    /// SHA-256 十六进制的前 8 位。
    fingerprint: String,
    /// 内存中的明文；从规范化 JSON 反序列化时为空。
    value: Option<T>,
}

impl<T: AsRef<[u8]>> Secret<T> {
    /// 由引用与明文构造，并计算指纹。
    #[must_use]
    pub fn new(key_ref: String, value: T) -> Self {
        let fingerprint = fingerprint_of(value.as_ref());
        Self {
            key_ref,
            fingerprint,
            value: Some(value),
        }
    }

    /// 密钥引用。
    #[must_use]
    pub fn secret_ref(&self) -> &str {
        &self.key_ref
    }

    /// 指纹（SHA-256 十六进制前 8 位）。
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// 明文（仅当持有时）。
    #[must_use]
    pub const fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }
}

/// SHA-256 十六进制前 8 个字符。
fn fingerprint_of(bytes: &[u8]) -> String {
    let hex = hex::encode(Sha256::digest(bytes));
    hex.get(..8).map_or_else(String::new, ToOwned::to_owned)
}

impl<T> Serialize for Secret<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut state = serializer.serialize_struct("Secret", 2)?;
        state.serialize_field("secret_ref", &self.key_ref)?;
        state.serialize_field("fingerprint", &self.fingerprint)?;
        state.end()
    }
}

impl<'de, T> Deserialize<'de> for Secret<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Envelope {
            /// 引用。
            secret_ref: String,
            /// 指纹。
            fingerprint: String,
        }
        let envelope = Envelope::deserialize(deserializer)?;
        Ok(Self {
            key_ref: envelope.secret_ref,
            fingerprint: envelope.fingerprint,
            value: None,
        })
    }
}

/// 超时配置文档（秒），与 [`Ttl`] 对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeouts {
    /// `Armed` 超时秒数。
    #[serde(rename = "arm_secs")]
    pub arm: u64,
    /// `Installing` 超时秒数。
    #[serde(rename = "install_secs")]
    pub install: u64,
    /// `Installed` 超时秒数。
    #[serde(rename = "firstboot_secs")]
    pub firstboot: u64,
    /// `Configuring` 超时秒数。
    #[serde(rename = "configure_secs")]
    pub configure: u64,
}

impl Timeouts {
    /// 由 [`Ttl`] 构造。
    ///
    /// # Errors
    ///
    /// 失败时返回对应错误。
    #[must_use]
    pub const fn from_ttl(ttl: Ttl) -> Self {
        Self {
            arm: ttl.arm().as_secs(),
            install: ttl.install().as_secs(),
            firstboot: ttl.firstboot().as_secs(),
            configure: ttl.configure().as_secs(),
        }
    }

    /// 转为运行时 [`Ttl`]。
    #[must_use]
    pub const fn to_ttl(self) -> Ttl {
        Ttl::from_secs(self.arm, self.install, self.firstboot, self.configure)
    }
}

impl ConfigDocument for Timeouts {
    const KIND: ConfigKind = ConfigKind::Timeouts;
    const SCHEMA: u32 = 1;

    fn validate(&self) -> Result<(), Vec<FieldError>> {
        Ok(())
    }

    fn migrate(from_schema: u32, value: Value) -> Result<Self, ConfigError> {
        migrate_current(from_schema, value)
    }
}

#[cfg(test)]
mod tests;
