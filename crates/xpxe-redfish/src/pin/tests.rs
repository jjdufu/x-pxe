//! 指纹解析与格式化。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::Fingerprint;

/// 冒号分隔、大写与 `sha256:` 前缀都能解析，输出为小写连续十六进制。
///
/// # Errors
///
/// 解析结果不符时返回错误。
#[test]
fn parse_forms() -> TestResult {
    let plain = Fingerprint::of_der(b"cert");
    let text = plain.to_string();
    ensure_eq(&text.len(), &64, "64 位")?;
    let colon: Vec<String> = text
        .as_bytes()
        .chunks(2)
        .map(|pair| String::from_utf8_lossy(pair).to_uppercase())
        .collect();
    ensure_eq(&Fingerprint::parse(&colon.join(":"))?, &plain, "冒号大写")?;
    ensure_eq(
        &Fingerprint::parse(&format!("sha256:{text}"))?,
        &plain,
        "前缀",
    )?;
    ensure(Fingerprint::parse("abcd").is_err(), "过短")?;
    ensure(Fingerprint::parse(&"zz".repeat(32)).is_err(), "非十六进制")?;
    Ok(())
}
