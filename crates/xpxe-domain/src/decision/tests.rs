//! 引导决策：Step 2 夹具回放，以及中继、非 PXE、过期、ARM64、HTTP Boot 关闭。

use std::fs;
use std::net::Ipv4Addr;

use serde::Deserialize;
use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{
    BootDirective, BootTarget, DhcpAnswer, DhcpRequestView, DhcpRules, DirectiveTable,
    FirmwareName, IgnoreReason, IpxeEntry, UefiFirmware, answer, ipxe_entry,
};
use crate::lifecycle::{NodeRuntime, NodeState, SessionId, Timestamp};
use crate::value::{BootToken, Hostname, HttpUrl, MacAddr};

/// 测试用当前时间。
const NOW: Timestamp = Timestamp::from_unix_seconds(1_000);
/// 指令过期时间（晚于 NOW）。
const EXPIRES: Timestamp = Timestamp::from_unix_seconds(2_000);
/// 已过期时间。
const PAST: Timestamp = Timestamp::from_unix_seconds(500);

/// 夹具 JSON 形状。
#[derive(Debug, Deserialize)]
struct Fixture {
    /// 来源文件名。
    source: String,
    /// MAC。
    mac: MacAddr,
    /// 中继地址。
    giaddr: Ipv4Addr,
    /// 选项 60。
    vendor_class: Option<String>,
    /// 选项 77。
    user_class: Option<String>,
    /// 选项 93。
    arch: Option<u16>,
    /// 选项 175。
    has_opt175: bool,
}

impl Fixture {
    /// 转为请求视图。
    fn view(&self) -> DhcpRequestView {
        DhcpRequestView {
            mac: self.mac,
            giaddr: self.giaddr,
            vendor_class: self.vendor_class.clone(),
            user_class: self.user_class.clone(),
            arch: self.arch,
            has_opt175: self.has_opt175,
        }
    }
}

/// 标准武装指令表。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn table_for(mac: MacAddr) -> TestResult<DirectiveTable> {
    Ok(DirectiveTable::new(
        HttpUrl::parse("http://10.77.0.1:8080")?,
        vec![BootDirective::new(
            mac,
            SessionId::parse("sess-1")?,
            BootToken::parse("testtoken")?,
            EXPIRES,
        )],
    ))
}

/// 读取全部 Step 2 请求夹具。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn load_fixtures() -> TestResult<Vec<Fixture>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut fixtures = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)?;
        fixtures.push(serde_json::from_str(&raw)?);
    }
    Ok(fixtures)
}

/// 在标准规则下对一次请求的期望。
fn expected(view: &DhcpRequestView) -> Expected<'_> {
    if view.giaddr != Ipv4Addr::UNSPECIFIED {
        return Expected::ignore(IgnoreReason::Relayed);
    }
    let vendor = view.vendor_class.as_deref().unwrap_or("");
    if !(vendor.starts_with("PXEClient") || vendor.starts_with("HTTPClient")) {
        return Expected::ignore(IgnoreReason::NotPxe);
    }
    if view.is_ipxe() {
        return Expected::http("http://10.77.0.1:8080/p/testtoken/boot.ipxe", false);
    }
    arch_expected(view)
}

/// 非 iPXE 的架构分支期望。
fn arch_expected(view: &DhcpRequestView) -> Expected<'_> {
    match view.client_arch() {
        crate::option93::ClientArch::BiosX86 => Expected::tftp(FirmwareName::UndionlyKpxe),
        crate::option93::ClientArch::UefiX64 => Expected::tftp(FirmwareName::IpxeEfi),
        crate::option93::ClientArch::UefiX64Http => {
            if view
                .vendor_class
                .as_deref()
                .is_some_and(|class| class.starts_with("HTTPClient"))
            {
                Expected::http("http://10.77.0.1:8080/fw/x64/ipxe.efi", true)
            } else {
                Expected::ignore(IgnoreReason::UnsupportedArch)
            }
        }
        crate::option93::ClientArch::Unsupported(_) => {
            Expected::ignore(IgnoreReason::UnsupportedArch)
        }
    }
}

/// 期望的应答（结构体而非枚举，避免变体体积差触发 lint）。
#[derive(Clone, Copy)]
struct Expected<'url> {
    /// 忽略原因。
    ignore: Option<IgnoreReason>,
    /// TFTP 文件。
    tftp: Option<FirmwareName>,
    /// HTTP URL。
    url: Option<&'url str>,
    /// 是否写 `HTTPClient`。
    http_client: bool,
}

impl Expected<'static> {
    /// 不应答。
    const fn ignore(reason: IgnoreReason) -> Self {
        Self {
            ignore: Some(reason),
            tftp: None,
            url: None,
            http_client: false,
        }
    }

    /// TFTP 固件。
    const fn tftp(file: FirmwareName) -> Self {
        Self {
            ignore: None,
            tftp: Some(file),
            url: None,
            http_client: false,
        }
    }
}

impl<'url> Expected<'url> {
    /// HTTP URL。
    const fn http(url: &'url str, http_client: bool) -> Self {
        Self {
            ignore: None,
            tftp: None,
            url: Some(url),
            http_client,
        }
    }
}

/// 比对决策结果。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn check_answer(source: &str, got: &DhcpAnswer, expect: Expected<'_>) -> TestResult {
    match (expect.ignore, expect.tftp, expect.url, got) {
        (Some(reason), None, None, DhcpAnswer::Ignore(got_reason)) => {
            Ok(ensure_eq(got_reason.as_ref(), &reason, source)?)
        }
        (None, Some(file), None, DhcpAnswer::Offer(offer)) => match &offer.target {
            BootTarget::Tftp(got_file) => Ok(ensure_eq(got_file, &file, source)?),
            BootTarget::Http(url) => Err(format!("{source} 期望 TFTP，得到 {url}").into()),
        },
        (None, None, Some(url), DhcpAnswer::Offer(offer)) => match &offer.target {
            BootTarget::Http(got_url) => {
                ensure_eq(&got_url.as_str(), &url, source)?;
                Ok(ensure_eq(
                    &offer.http_client_class,
                    &expect.http_client,
                    source,
                )?)
            }
            BootTarget::Tftp(file) => {
                Err(format!("{source} 期望 HTTP，得到 {}", file.as_str()).into())
            }
        },
        (_, _, _, _) => {
            Err(format!("{source} 期望与实际不符：{got:?} / {}", describe(&expect)).into())
        }
    }
}

/// 期望的简短说明。
fn describe(expect: &Expected<'_>) -> String {
    if let Some(reason) = expect.ignore {
        return format!("Ignore({reason:?})");
    }
    if let Some(file) = expect.tftp {
        return format!("Tftp({})", file.as_str());
    }
    if let Some(url) = expect.url {
        return format!("Http({url}, {})", expect.http_client);
    }
    "empty".to_owned()
}

/// 所有 Step 2 请求夹具都有对应决策断言。
///
/// # Errors
///
/// 夹具缺失或决策不符时返回错误。
#[test]
fn all_step2_fixtures_have_answers() -> TestResult {
    let fixtures = load_fixtures()?;
    ensure(fixtures.len() >= 70, "夹具数量不足")?;
    let rules = DhcpRules::standard();
    for fixture in &fixtures {
        let view = fixture.view();
        let table = table_for(view.mac)?;
        let got = answer(&view, &table, rules, NOW);
        check_answer(&fixture.source, &got, expected(&view))?;
    }
    Ok(())
}

/// 构造一个已武装的 BIOS PXE 请求。
fn pxe_bios(mac: MacAddr) -> DhcpRequestView {
    DhcpRequestView {
        mac,
        giaddr: Ipv4Addr::UNSPECIFIED,
        vendor_class: Some("PXEClient:Arch:00000:UNDI:002001".to_owned()),
        user_class: None,
        arch: Some(0),
        has_opt175: false,
    }
}

/// 中继、未武装、过期、非 PXE、ARM64、HTTP Boot 关闭。
///
/// # Errors
///
/// 某条路径未按原因忽略时返回错误。
#[test]
fn ignore_reasons() -> TestResult {
    let mac = MacAddr::parse("52:54:00:77:01:b3")?;
    let table = table_for(mac)?;
    let rules = DhcpRules::standard();
    let mut relayed = pxe_bios(mac);
    relayed.giaddr = Ipv4Addr::new(192, 168, 1, 1);
    ensure_ignore(&relayed, &table, rules, IgnoreReason::Relayed, "中继")?;
    let unknown = pxe_bios(MacAddr::parse("00:00:00:00:00:01")?);
    ensure_ignore(&unknown, &table, rules, IgnoreReason::NotArmed, "未武装")?;
    let expired_table = DirectiveTable::new(
        HttpUrl::parse("http://10.77.0.1:8080")?,
        vec![BootDirective::new(
            mac,
            SessionId::parse("sess-1")?,
            BootToken::parse("testtoken")?,
            PAST,
        )],
    );
    ensure_ignore(
        &pxe_bios(mac),
        &expired_table,
        rules,
        IgnoreReason::Expired,
        "过期",
    )?;
    let mut not_pxe = pxe_bios(mac);
    not_pxe.vendor_class = Some("d-i".to_owned());
    ensure_ignore(&not_pxe, &table, rules, IgnoreReason::NotPxe, "非 PXE")?;
    let mut arm64 = pxe_bios(mac);
    arm64.vendor_class = Some("PXEClient:Arch:00011".to_owned());
    arm64.arch = Some(11);
    ensure_ignore(
        &arm64,
        &table,
        rules,
        IgnoreReason::UnsupportedArch,
        "ARM64",
    )?;
    http_boot_disabled(mac, &table)
}

/// HTTP Boot 关闭时忽略 arch 16 / `HTTPClient`。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn http_boot_disabled(mac: MacAddr, table: &DirectiveTable) -> TestResult {
    let rules = DhcpRules::standard().with_http_boot(false);
    let req = DhcpRequestView {
        mac,
        giaddr: Ipv4Addr::UNSPECIFIED,
        vendor_class: Some("HTTPClient:Arch:00016:UNDI:003001".to_owned()),
        user_class: None,
        arch: Some(16),
        has_opt175: false,
    };
    ensure_ignore(
        &req,
        table,
        rules,
        IgnoreReason::HttpBootDisabled,
        "HTTP 关闭",
    )
}

/// 断言 `answer` 返回指定忽略原因。
///
/// # Errors
///
/// 断言失败或测试数据不合法时返回错误。
fn ensure_ignore(
    req: &DhcpRequestView,
    table: &DirectiveTable,
    rules: DhcpRules,
    reason: IgnoreReason,
    label: &str,
) -> TestResult {
    match answer(req, table, rules, NOW) {
        DhcpAnswer::Ignore(got) => Ok(ensure_eq(got.as_ref(), &reason, label)?),
        DhcpAnswer::Offer(_) => Err(format!("{label} 不应要约").into()),
    }
}

/// UEFI 规则可切换 `snponly.efi`；选项 93 = 9 视为 UEFI x64。
///
/// # Errors
///
/// 固件选择不符时返回错误。
#[test]
fn uefi_firmware_choice_and_arch9() -> TestResult {
    let mac = MacAddr::parse("52:54:00:77:01:b3")?;
    let table = table_for(mac)?;
    let mut req = pxe_bios(mac);
    req.vendor_class = Some("PXEClient:Arch:00007".to_owned());
    req.arch = Some(9);
    let rules = DhcpRules::standard().with_uefi_firmware(UefiFirmware::SnpOnlyEfi);
    check_answer(
        "arch9-snp",
        &answer(&req, &table, rules, NOW),
        Expected::tftp(FirmwareName::SnpOnlyEfi),
    )
}

/// iPXE 入口仅 `Armed` 返回链式加载。
///
/// # Errors
///
/// 入口决策不符时返回错误。
#[test]
fn ipxe_entry_only_when_armed() -> TestResult {
    let mac = MacAddr::parse("52:54:00:77:01:b3")?;
    let hostname = Hostname::parse("node1")?;
    let idle = NodeRuntime::idle(hostname, mac, NOW);
    ensure_eq(
        &ipxe_entry(mac, &idle),
        &IpxeEntry::Exit(NodeState::Idle),
        "idle",
    )?;
    Ok(())
}
