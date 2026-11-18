//! 静态配置解析。

use std::collections::BTreeMap;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::Settings;

/// 由键值表构造查找函数并解析。
///
/// # Errors
///
/// 失败时返回错误。
fn parse(pairs: &[(&str, &str)]) -> Result<Settings, crate::Failure> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    Settings::from_lookup(|key| map.get(key).cloned())
}

/// 默认值、必填项与证书成对约束。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn defaults_and_tls_pairing() -> TestResult {
    let base = ("XPXE_BOOT_BASE_URL", "http://10.77.0.1:8080/");
    let settings = parse(&[base])?;
    ensure_eq(
        &settings.boot_base.as_str(),
        &"http://10.77.0.1:8080",
        "去掉末尾斜杠",
    )?;
    ensure_eq(&settings.admin_listen.port(), &8088, "管理端默认端口")?;
    ensure(settings.tls.is_none(), "默认 HTTP")?;
    ensure(parse(&[]).is_err(), "缺少引导地址")?;
    ensure(
        parse(&[base, ("XPXE_TLS_CERT_FILE", "/c.pem")]).is_err(),
        "只有证书",
    )?;
    ensure(
        parse(&[base, ("XPXE_TLS_KEY_FILE", "/k.pem")]).is_err(),
        "只有私钥",
    )?;
    let tls = parse(&[
        base,
        ("XPXE_TLS_CERT_FILE", "/c"),
        ("XPXE_TLS_KEY_FILE", "/k"),
    ])?;
    ensure(tls.tls.is_some(), "两者都有时启用")?;
    Ok(())
}
