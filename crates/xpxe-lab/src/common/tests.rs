//! 公共工具测试。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{Args, format_mac, in_lab_net, parse_mac};

/// 参数解析：键值、重复键、开关与默认值。
///
/// # Errors
///
/// 解析结果不符时返回错误。
#[test]
fn parses_args() -> TestResult {
    let args = Args::new(
        [
            "--iface",
            "br-lab",
            "--allow-mac",
            "a",
            "--allow-mac",
            "b",
            "--uefi",
            "--wait",
            "3",
        ]
        .iter()
        .map(|item| (*item).to_owned())
        .collect(),
    );
    ensure_eq(&args.required("--iface")?, &"br-lab".to_owned(), "必填")?;
    ensure_eq(
        &args.values("--allow-mac"),
        &vec!["a".to_owned(), "b".to_owned()],
        "重复键",
    )?;
    ensure(args.flag("--uefi"), "开关")?;
    ensure_eq(&args.parsed("--wait", 1_u64)?, &3, "数值")?;
    ensure_eq(&args.parsed("--missing", 7_u64)?, &7, "默认值")?;
    ensure(args.required("--missing").is_err(), "缺失必填")?;
    Ok(())
}

/// MAC 解析与格式化。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn mac_round_trip() -> TestResult {
    let mac = parse_mac("52:54:00:77:01:0A")?;
    ensure_eq(&format_mac(mac), &"52:54:00:77:01:0a".to_owned(), "往返")?;
    ensure(parse_mac("52:54:00").is_err(), "长度")?;
    ensure(parse_mac("zz:54:00:77:01:0a").is_err(), "字符")?;
    Ok(())
}

/// 路由表地址判定。
///
/// # Errors
///
/// 判定不符时返回错误。
#[test]
fn lab_net_detection() -> TestResult {
    ensure(in_lab_net("01004D0A"), "10.77.0.1")?;
    ensure(!in_lab_net("011E10AC"), "172.30.0.1")?;
    ensure(!in_lab_net("zz"), "非法")?;
    Ok(())
}
