//! `settings` 模块单元测试。

use std::collections::BTreeMap;

use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

use super::{Settings, Token};

/// 环境解析：默认值、白名单、URL 与令牌校验。
///
/// # Errors
///
/// 断言失败。
#[test]
fn parses_environment() -> TestResult {
    let dir = TempDataDir::new()?;
    let token_path = dir.path().join("token");
    std::fs::write(&token_path, "s3cret-token\n")?;
    let token_file = token_path.to_string_lossy().into_owned();
    let mut env = BTreeMap::new();
    let _old = env.insert("XPXE_BOOTNET_TOKEN_FILE", token_file);
    let _old2 = env.insert("XPXE_BOOTNET_ALLOWED_IFACES", "br-lab, ens192".to_owned());
    let settings = Settings::from_lookup(|key| env.get(key).cloned())?;
    ensure_eq(
        &settings.control_url.as_str(),
        &"http://127.0.0.1:8081",
        "默认控制面",
    )?;
    ensure_eq(&settings.allowed_ifaces.len(), &2, "两个网卡")?;
    ensure_eq(&settings.token.expose(), &"s3cret-token", "令牌去空白")?;
    ensure(
        !format!("{:?}", settings.token).contains("s3cret"),
        "Debug 不泄露令牌",
    )?;

    let _url = env.insert("XPXE_CONTROL_URL", "https://x:1".to_owned());
    ensure(
        Settings::from_lookup(|key| env.get(key).cloned()).is_err(),
        "拒绝 https",
    )?;
    let _url2 = env.insert("XPXE_CONTROL_URL", "http://10.77.0.1:8081/".to_owned());
    let custom = Settings::from_lookup(|key| env.get(key).cloned())?;
    ensure_eq(
        &custom.control_url.as_str(),
        &"http://10.77.0.1:8081",
        "去掉末尾 /",
    )?;
    let _ifaces = env.insert("XPXE_BOOTNET_ALLOWED_IFACES", " , ".to_owned());
    ensure(
        Settings::from_lookup(|key| env.get(key).cloned()).is_err(),
        "空白名单",
    )?;
    let _ifaces2 = env.insert("XPXE_BOOTNET_ALLOWED_IFACES", "bad iface".to_owned());
    ensure(
        Settings::from_lookup(|key| env.get(key).cloned()).is_err(),
        "非法网卡名",
    )?;
    let _missing = env.insert(
        "XPXE_BOOTNET_TOKEN_FILE",
        dir.path().join("none").to_string_lossy().into_owned(),
    );
    ensure(
        Settings::from_lookup(|key| env.get(key).cloned()).is_err(),
        "令牌文件缺失",
    )?;
    ensure(Token::parse("  ").is_err(), "空令牌")?;
    ensure(Token::parse("a b").is_err(), "含空格")?;
    Ok(())
}
