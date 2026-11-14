//! 值类型合法 / 非法样例：换行、`$()`、反引号、`;`、NUL、超长。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{
    BootToken, Fqdn, Hostname, HttpUrl, Ipv4Cidr, Locale, MacAddr, PackageName, ParseError,
    SafeText, SshPublicKey, TimeZone, UnixUser,
};

/// 解析失败且错误码匹配。
/// 断言解析以给定错误码失败。
///
/// # Errors
///
/// 输入被接受或错误码不符时返回错误。
fn err_code<T>(result: &Result<T, ParseError>, typ: &str, code: &str) -> TestResult {
    match result {
        Ok(_) => Err(format!("{typ} 应拒绝该输入").into()),
        Err(error) => {
            ensure_eq(&error.typ(), &typ, "类型名")?;
            ensure_eq(&error.code(), &code, "错误码")?;
            Ok(())
        }
    }
}

/// MAC 三种格式与两种输出。
///
/// # Errors
///
/// 解析或格式化不符时返回错误。
#[test]
fn mac_accepts_three_formats() -> TestResult {
    let colon = MacAddr::parse("52:54:00:77:01:B3")?;
    let hyphen = MacAddr::parse("52-54-00-77-01-b3")?;
    let compact = MacAddr::parse("5254007701b3")?;
    ensure_eq(&colon, &hyphen, "冒号与连字符")?;
    ensure_eq(&colon, &compact, "紧凑形式")?;
    ensure_eq(colon.to_colon().as_str(), "52:54:00:77:01:b3", "colon")?;
    ensure_eq(colon.to_hexhyp().as_str(), "52-54-00-77-01-b3", "hexhyp")?;
    let json = serde_json::to_string(&colon)?;
    ensure_eq(json.as_str(), "\"52:54:00:77:01:b3\"", "serde")?;
    ensure_eq(&serde_json::from_str::<MacAddr>(&json)?, &colon, "de")?;
    Ok(())
}

/// 非法 MAC：分隔符混用、过短、含换行。
///
/// # Errors
///
/// 某输入被接受时返回错误。
#[test]
fn mac_rejects_malformed() -> TestResult {
    err_code(
        &MacAddr::parse("52:54:00-77:01:b3"),
        "MacAddr",
        "bad_format",
    )?;
    err_code(&MacAddr::parse("52:54:00:77:01"), "MacAddr", "bad_format")?;
    err_code(
        &MacAddr::parse("52:54:00:77:01:b3\n"),
        "MacAddr",
        "bad_format",
    )?;
    err_code(&MacAddr::parse(""), "MacAddr", "bad_format")?;
    Ok(())
}

/// 主机名规范为小写；FQDN 去掉尾点。
///
/// # Errors
///
/// 合法样例被拒或非法样例被接受时返回错误。
#[test]
fn hostname_and_fqdn_rules() -> TestResult {
    ensure_eq(&Hostname::parse("Node-1")?.as_str(), &"node-1", "小写")?;
    err_code(&Hostname::parse(""), "Hostname", "empty")?;
    err_code(&Hostname::parse("-node"), "Hostname", "bad_format")?;
    err_code(&Hostname::parse("node_1"), "Hostname", "bad_format")?;
    err_code(&Hostname::parse("n$(id)"), "Hostname", "bad_format")?;
    err_code(&Hostname::parse("n`id`"), "Hostname", "bad_format")?;
    err_code(&Hostname::parse("n;id"), "Hostname", "bad_format")?;
    err_code(&Hostname::parse(&"a".repeat(64)), "Hostname", "too_long")?;
    ensure_eq(
        &Fqdn::parse("Node.Example.COM.")?.as_str(),
        &"node.example.com",
        "fqdn",
    )?;
    err_code(&Fqdn::parse("bad_label.example"), "Fqdn", "bad_format")?;
    err_code(&Fqdn::parse("."), "Fqdn", "empty")?;
    Ok(())
}

/// IPv4 CIDR 允许主机地址，拒绝越界前缀。
///
/// # Errors
///
/// 样例不符时返回错误。
#[test]
fn ipv4_cidr_rules() -> TestResult {
    let cidr = Ipv4Cidr::parse("192.168.2.10/24")?;
    ensure_eq(cidr.to_string().as_str(), "192.168.2.10/24", "display")?;
    err_code(&Ipv4Cidr::parse("192.168.2.10"), "Ipv4Cidr", "bad_format")?;
    err_code(
        &Ipv4Cidr::parse("192.168.2.10/33"),
        "Ipv4Cidr",
        "bad_prefix",
    )?;
    err_code(
        &Ipv4Cidr::parse("192.168.2.10/24\n"),
        "Ipv4Cidr",
        "bad_format",
    )?;
    Ok(())
}

/// Unix 用户：`root` 与普通名允许，系统保留名与注入字符拒绝。
///
/// # Errors
///
/// 样例不符时返回错误。
#[test]
fn unix_user_rules() -> TestResult {
    ensure_eq(&UnixUser::parse("root")?.as_str(), &"root", "root")?;
    ensure_eq(
        &UnixUser::parse("jwczjzl")?.as_str(),
        &"jwczjzl",
        "普通用户",
    )?;
    err_code(&UnixUser::parse("daemon"), "UnixUser", "reserved")?;
    err_code(&UnixUser::parse("nobody"), "UnixUser", "reserved")?;
    err_code(&UnixUser::parse("_apt"), "UnixUser", "reserved")?;
    err_code(&UnixUser::parse("Root"), "UnixUser", "bad_format")?;
    err_code(&UnixUser::parse("n$(id)"), "UnixUser", "bad_format")?;
    err_code(&UnixUser::parse("n;id"), "UnixUser", "bad_format")?;
    err_code(&UnixUser::parse(""), "UnixUser", "empty")?;
    Ok(())
}

/// Debian 包名规则与注入字符。
///
/// # Errors
///
/// 样例不符时返回错误。
#[test]
fn package_name_rules() -> TestResult {
    ensure_eq(&PackageName::parse("g++")?.as_str(), &"g++", "g++")?;
    ensure_eq(&PackageName::parse("libssl3")?.as_str(), &"libssl3", "lib")?;
    err_code(&PackageName::parse("A"), "PackageName", "too_short")?;
    err_code(&PackageName::parse("Bad"), "PackageName", "bad_format")?;
    err_code(&PackageName::parse("pkg;rm"), "PackageName", "bad_format")?;
    err_code(&PackageName::parse("pkg`id`"), "PackageName", "bad_format")?;
    err_code(
        &PackageName::parse(&"a".repeat(256)),
        "PackageName",
        "too_long",
    )?;
    Ok(())
}

/// 已知 SSH 公钥样例。
const ED25519: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIOaMuk7v05hBqpNz4AEjdxbT4j/jbXd2W3rFthvWmR1 ok-comment";
/// ECDSA P-256。
const ECDSA: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBMWv6Fj4MBsgzJWJU6txzpzfhQv9FQse53j586hmteP5k4MnkXKqJZaIzQ/W67NBu8q5dF6xD8WZxjlXDg6Hfjc= ok";
/// RSA 3072。
const RSA_3072: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCxlsbzfDBSrV0MWdRgsML0L/AhAD4+dUfwkY1V9pHiyP63lRv8CxiULG127z0qr1Sy6FQbLsLO3pB8Bf+nWaOj7VV9HpyNe3Rb/ApfhFwnhDpx31Jj5Iq54ki1nx8GiO+VvKHbO7Vlu+2O6H0DM5jPAFmSfdnWnCbZSKPs2j0i4YAARVZHA7ehbiew2aKXiBN13c8164ji1IB+SOakp82m7kSDKNTmp2TeNATD7f3mWDMgHSv65za9AMwRg90zIUbodmROs9Wj2bPrhVpf1wVggQhPxfjyYSxoNlOkxk43N6X5VmdrkD371Iz9xfRTLWF6cOUXJR6viWAPTKuvckYt23zTPUtj5/SWRDVFwot5Xa7C8kgjAEp2Da1GVvJn/3sqaNN+6HianyZl+tktidZafkH1uuKJ2hjx03tH1ACK0eb14fHQXR5eQ0vKwl6Cytqhu7xW+pVJzslwParE3ShhRKA9UHSV2vJrNc+aQWUgNbKvo50dhSByZaG0RsGu01U= ok";
/// RSA 2048，应拒绝。
const RSA_2048: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDXJ6GyKrGP4N17QvK269hFxLvGWPjVp/Lt3v2cC7dO79JT/cFfXYpt8/yh8oYsFyiKIodbdD2/p0+j45rwmZzz7MgKCZpCyTCzwltzB6E4lAgosKYkBGmAf20izm/cBkPHxwZRujXiRETgDj+GxGZB3haggvy96GyWgd2t+DfrpjcZSKN4otkRNEv90JCMDlIsq8Z2WSGhvWHawydpMdv01bmHWxmMoar6zancR+2BxZQk4JwY9CtyRRZRMBOWg4HO8rp9oqdBAB+4TK7jYGc9+VdiSSarr9GMzsGhO1nXwV/ZlrKOgrUzMyhDOXVYQW8bDtGVm+dq2Eo8qjPT6c3D short";
/// 构造的 sk-ed25519。
const SK_ED25519: &str = "sk-ssh-ed25519@openssh.com AAAAGnNrLXNzaC1lZDI1NTE5QG9wZW5zc2guY29tAAAAIBERERERERERERERERERERERERERERERERERERERERERAAAABHNzaDo= sk-ok";

/// 白名单类型的合法公钥被接受。
///
/// # Errors
///
/// 合法公钥被拒时返回错误。
#[test]
fn ssh_keys_accepted() -> TestResult {
    ensure(SshPublicKey::parse(ED25519)?.as_str() == ED25519, "ed")?;
    let _ecdsa = SshPublicKey::parse(ECDSA)?;
    let _rsa = SshPublicKey::parse(RSA_3072)?;
    let _sk = SshPublicKey::parse(SK_ED25519)?;
    Ok(())
}

/// RSA 过短、类型不一致、换行、非法注释均拒绝。
///
/// # Errors
///
/// 非法公钥被接受时返回错误。
#[test]
fn ssh_keys_rejected() -> TestResult {
    err_code(
        &SshPublicKey::parse(RSA_2048),
        "SshPublicKey",
        "rsa_too_short",
    )?;
    let mismatched = format!(
        "ssh-rsa {}",
        ED25519.split_once(' ').map_or("", |parts| parts.1)
    );
    err_code(
        &SshPublicKey::parse(&mismatched),
        "SshPublicKey",
        "type_mismatch",
    )?;
    err_code(
        &SshPublicKey::parse(&format!("{ED25519}\n")),
        "SshPublicKey",
        "control_char",
    )?;
    err_code(
        &SshPublicKey::parse(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIOaMuk7v05hBqpNz4AEjdxbT4j/jbXd2W3rFthvWmR1 注释中文",
        ),
        "SshPublicKey",
        "bad_comment",
    )?;
    err_code(
        &SshPublicKey::parse("ssh-dss AAAA"),
        "SshPublicKey",
        "bad_type",
    )?;
    Ok(())
}

/// 时区与 locale 必须在清单中精确匹配。
///
/// # Errors
///
/// 样例不符时返回错误。
#[test]
fn zone_and_locale_lists() -> TestResult {
    ensure_eq(
        &TimeZone::parse("Asia/Shanghai")?.as_str(),
        &"Asia/Shanghai",
        "tz",
    )?;
    let _utc = TimeZone::parse("UTC")?;
    let _etc = TimeZone::parse("Etc/UTC")?;
    err_code(&TimeZone::parse("asia/shanghai"), "TimeZone", "unknown")?;
    err_code(&TimeZone::parse("Foo/Bar"), "TimeZone", "unknown")?;
    ensure_eq(
        &Locale::parse("en_US.UTF-8")?.as_str(),
        &"en_US.UTF-8",
        "en",
    )?;
    ensure_eq(
        &Locale::parse("zh_CN.UTF-8")?.as_str(),
        &"zh_CN.UTF-8",
        "zh",
    )?;
    err_code(&Locale::parse("en_US.utf8"), "Locale", "unknown")?;
    Ok(())
}

/// HTTP(S) URL 必须有主机，拒绝空白、控制字符与其他方案。
///
/// # Errors
///
/// 样例不符时返回错误。
#[test]
fn http_url_rules() -> TestResult {
    let url = HttpUrl::parse("HTTP://10.77.0.1:8080/p/tok/boot.ipxe")?;
    ensure_eq(
        &url.as_str(),
        &"http://10.77.0.1:8080/p/tok/boot.ipxe",
        "norm",
    )?;
    err_code(
        &HttpUrl::parse("ftp://example.com"),
        "HttpUrl",
        "bad_scheme",
    )?;
    err_code(&HttpUrl::parse("http://"), "HttpUrl", "bad_host")?;
    err_code(
        &HttpUrl::parse("http://user:pass@host/"),
        "HttpUrl",
        "bad_host",
    )?;
    err_code(&HttpUrl::parse("http://ex ample"), "HttpUrl", "bad_char")?;
    err_code(
        &HttpUrl::parse("http://example.com/a\n"),
        "HttpUrl",
        "bad_char",
    )?;
    Ok(())
}

/// `SafeText` 允许可打印注入字符，拒绝控制字符与超长。
///
/// # Errors
///
/// 样例不符时返回错误。
#[test]
fn safe_text_rules() -> TestResult {
    ensure_eq(
        &SafeText::parse("ok $(id); `rm`")?.as_str(),
        &"ok $(id); `rm`",
        "printable",
    )?;
    err_code(&SafeText::parse("line\nbreak"), "SafeText", "control_char")?;
    err_code(&SafeText::parse("nul\0x"), "SafeText", "control_char")?;
    err_code(&SafeText::parse(""), "SafeText", "empty")?;
    err_code(
        &SafeText::parse(&"a".repeat(SafeText::MAX_LEN.saturating_add(1))),
        "SafeText",
        "too_long",
    )?;
    let _token = BootToken::parse("testtoken")?;
    err_code(&BootToken::parse("short"), "BootToken", "too_short")?;
    err_code(&BootToken::parse("bad/token"), "BootToken", "bad_format")?;
    Ok(())
}

/// 脏数据经 serde 同样被拒绝。
///
/// # Errors
///
/// 脏 JSON 被接受时返回错误。
#[test]
fn serde_rejects_dirty_data() -> TestResult {
    let hostname = serde_json::from_str::<Hostname>("\"n $(id)\"");
    ensure(hostname.is_err(), "serde Hostname")?;
    let user = serde_json::from_str::<UnixUser>("\"daemon\"");
    ensure(user.is_err(), "serde UnixUser")?;
    Ok(())
}
