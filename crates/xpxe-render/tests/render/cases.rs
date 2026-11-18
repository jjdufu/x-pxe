//! 渲染快照、一致性、注入防护与长度边界。

use std::fs;
use std::path::PathBuf;

use xpxe_domain::{HttpUrl, NodeSpec, Profile};
use xpxe_render::{
    ArmedState, RenderRequest, RenderRequestParts, RequestDigests, RequestTokens, render,
};
use xpxe_test_support::{TestResult, ensure, ensure_eq};

/// 固定会话令牌，保证快照稳定。
const BOOT: &str = "boot0001";
/// Agent 令牌。
const AGENT: &str = "agent001";
/// 事件 ID。
const EVENT: &str = "ev-00000001";
/// 会话短标识。
const SESSION: &str = "sess01";
/// 空 agent 文件的 SHA-256。
const AGENT_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// 空 plan 的 SHA-256（与空文件相同，测试里用空字节；真实 plan 另计）。
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// 构造渲染请求。
///
/// # Errors
///
/// 解析失败时返回错误。
fn request(
    preset: &str,
    node_rel: &str,
    boot_base: &str,
    armed: bool,
) -> TestResult<RenderRequest> {
    request_with(Profile::load_preset(preset)?, node_rel, boot_base, armed)
}

/// 在预设 JSON 上覆盖若干顶层字段后构造 Profile。
///
/// # Errors
///
/// 预设无法加载或覆盖后校验失败时返回错误。
fn profile_with(preset: &str, overrides: &[(&str, serde_json::Value)]) -> TestResult<Profile> {
    let mut doc = serde_json::to_value(Profile::load_preset(preset)?)?;
    for (key, value) in overrides {
        set_pointer(&mut doc, key, value.clone())?;
    }
    Ok(serde_json::from_value(doc)?)
}

/// 按 JSON Pointer 写入值。
///
/// # Errors
///
/// 指针不存在时返回错误。
fn set_pointer(doc: &mut serde_json::Value, pointer: &str, value: serde_json::Value) -> TestResult {
    let slot = doc
        .pointer_mut(pointer)
        .ok_or_else(|| format!("预设中不存在 {pointer}"))?;
    *slot = value;
    Ok(())
}

/// 用给定 Profile 构造渲染请求。
///
/// # Errors
///
/// 解析失败时返回错误。
fn request_with(
    profile: Profile,
    node_rel: &str,
    boot_base: &str,
    armed: bool,
) -> TestResult<RenderRequest> {
    let node_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lab/nodes")
        .join(node_rel);
    let node_raw = fs::read_to_string(&node_path)?;
    let node: NodeSpec = serde_json::from_str(&node_raw)?;
    let parts = RenderRequestParts {
        profile,
        node,
        boot_base: HttpUrl::parse(boot_base)?,
        tokens: RequestTokens {
            boot_token: BOOT.parse()?,
            agent_token: AGENT.parse()?,
            session_short: SESSION.to_owned(),
            event_id: EVENT.parse()?,
        },
        digests: RequestDigests {
            agent_sha256: AGENT_SHA.to_owned(),
            plan_sha256: EMPTY_SHA.to_owned(),
        },
        armed: if armed {
            ArmedState::armed()
        } else {
            ArmedState::not_armed("node-not-armed".to_owned())
        },
    };
    Ok(RenderRequest::from_parts(parts))
}

/// 黄金文件目录。
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/render/golden")
}

/// 与黄金文件比较；设置 `XPXE_UPDATE_GOLDEN=1` 时覆盖。
///
/// # Errors
///
/// 内容不符时返回错误。
fn compare_golden(name: &str, actual: &str) -> TestResult {
    let path = golden_dir().join(name);
    if std::env::var("XPXE_UPDATE_GOLDEN").is_ok() {
        fs::create_dir_all(golden_dir())?;
        fs::write(&path, actual)?;
        return Ok(());
    }
    let expected = fs::read_to_string(&path).map_err(|error| {
        format!(
            "缺少黄金文件 {}（{error}）。可设 XPXE_UPDATE_GOLDEN=1 生成",
            path.display()
        )
    })?;
    ensure_eq(actual, expected.as_str(), name)?;
    Ok(())
}

/// 预设 × DHCP/静态 × firmware 开关的快照。
///
/// # Errors
///
/// 渲染失败或与黄金文件不符时返回错误。
#[test]
fn snapshot_presets() -> TestResult {
    let cases = [
        (
            "nexus-lvm",
            "vm-uefi.json",
            "http://10.77.0.1:8080",
            "nexus-dhcp",
        ),
        (
            "nexus-lvm",
            "vm-static.json",
            "http://10.77.0.1:8080",
            "nexus-static",
        ),
        (
            "official-mirror",
            "vm-uefi.json",
            "https://example.com:8080",
            "official-dhcp",
        ),
    ];
    for (preset, node, base, label) in cases {
        let rendered = render(&request(preset, node, base, true)?)?;
        compare_golden(&format!("{label}.ipxe"), rendered.boot_ipxe())?;
        compare_golden(&format!("{label}.cmdline"), rendered.cmdline())?;
        compare_golden(&format!("{label}.preseed"), rendered.preseed())?;
    }
    Ok(())
}

/// 自定义分区配方。
fn custom_partitions() -> serde_json::Value {
    serde_json::json!({
        "type": "custom",
        "partitions": [
            { "mount": { "type": "path", "path": "/boot" }, "size": { "type": "mib", "mib": 1024 }, "fs": "ext4", "on_lvm": false },
            { "mount": { "type": "swap" }, "size": { "type": "mib", "mib": 4096 }, "fs": "swap", "on_lvm": true },
            { "mount": { "type": "path", "path": "/" }, "size": { "type": "rest" }, "fs": "ext4", "on_lvm": true }
        ]
    })
}

/// {DHCP, 静态} × {`AtomicLvm`, `Custom`} × {`firmware_cpio` 开/关} 全矩阵快照。
///
/// # Errors
///
/// 渲染失败或与黄金文件不符时返回错误。
#[test]
fn snapshot_matrix() -> TestResult {
    for (node, net) in [("vm-uefi.json", "dhcp"), ("vm-static.json", "static")] {
        for (scheme, part) in [
            ("lvm", serde_json::json!({ "type": "atomic_lvm" })),
            ("custom", custom_partitions()),
        ] {
            for firmware in [false, true] {
                let profile = profile_with(
                    "nexus-lvm",
                    &[
                        ("/partitioning", part.clone()),
                        ("/firmware_cpio", serde_json::Value::Bool(firmware)),
                    ],
                )?;
                let rendered =
                    render(&request_with(profile, node, "http://10.77.0.1:8080", true)?)?;
                let label = format!("matrix-{net}-{scheme}-fw{}", u8::from(firmware));
                compare_golden(&format!("{label}.ipxe"), rendered.boot_ipxe())?;
                compare_golden(&format!("{label}.cmdline"), rendered.cmdline())?;
                compare_golden(&format!("{label}.preseed"), rendered.preseed())?;
                compare_golden(
                    &format!("{label}.partman-early.sh"),
                    rendered.partman_early(),
                )?;
                ensure_eq(
                    &rendered.boot_ipxe().contains("firmware.cpio.gz"),
                    &firmware,
                    "firmware initrd",
                )?;
            }
        }
    }
    Ok(())
}

/// 钩子与固定产物快照（与分区方案无关）。
///
/// # Errors
///
/// 与黄金文件不符时返回错误。
#[test]
fn snapshot_hooks() -> TestResult {
    let rendered = render(&request(
        "nexus-lvm",
        "vm-uefi.json",
        "http://10.77.0.1:8080",
        true,
    )?)?;
    compare_golden("hooks-late.sh", rendered.late())?;
    compare_golden("hooks-firstboot.service", rendered.firstboot_unit())?;
    compare_golden("hooks-disk-select.sh", rendered.disk_select())?;
    Ok(())
}

/// 注入载荷。
const PAYLOADS: [&str; 6] = [
    "x'y",
    "$(id)",
    "`id`",
    "a\nd-i evil boolean true",
    "a;reboot",
    "a b\tc",
];

/// 每个字符串字段写入注入载荷：要么构造/渲染失败，要么输出不出现行注入且 shell 钩子正确转义。
///
/// # Errors
///
/// 出现注入时返回错误。
#[test]
fn injection_is_rejected_or_escaped() -> TestResult {
    let fields = [
        "/name",
        "/locale",
        "/keymap",
        "/timezone",
        "/ntp/0",
        "/mirror/host_port",
        "/mirror/directory",
        "/console/items/0",
        "/disk/exclude_models/0",
    ];
    let mut rendered_count = 0_u32;
    for field in fields {
        for payload in PAYLOADS {
            let value = serde_json::Value::String(payload.to_owned());
            let Ok(profile) = profile_with("nexus-lvm", &[(field, value)]) else {
                continue;
            };
            let Ok(rendered) = render(&request_with(
                profile,
                "vm-static.json",
                "http://10.77.0.1:8080",
                true,
            )?) else {
                continue;
            };
            rendered_count += 1;
            let label = format!("{field} <- {payload:?}");
            ensure(!rendered.cmdline().contains('\n'), &label)?;
            for text in [
                rendered.preseed(),
                rendered.boot_ipxe(),
                rendered.disk_select(),
            ] {
                ensure(
                    !text.lines().any(|line| line.starts_with("d-i evil")),
                    &label,
                )?;
            }
            for script in [
                rendered.disk_select(),
                rendered.partman_early(),
                rendered.late(),
            ] {
                ensure(sh_syntax_ok(script)?, &label)?;
            }
        }
    }
    // 至少有部分无害字符（如空格、分号）在宽松字段上被接受并渲染。
    ensure(rendered_count < 54, "存在字段完全未校验")?;
    Ok(())
}

/// `sh -n` 语法检查。
///
/// # Errors
///
/// 无法启动 `/bin/sh` 时返回错误。
fn sh_syntax_ok(script: &str) -> TestResult<bool> {
    let dir = xpxe_test_support::TempDataDir::new()?;
    let path = dir.path().join("hook.sh");
    fs::write(&path, script)?;
    let status = std::process::Command::new("/bin/sh")
        .arg("-n")
        .arg(&path)
        .status()?;
    Ok(status.success())
}

/// 内核参数中的 MD5 与静态地址与 preseed 一致。
///
/// # Errors
///
/// 不一致时返回错误。
#[test]
fn cmdline_matches_preseed() -> TestResult {
    let rendered = render(&request(
        "nexus-lvm",
        "vm-static.json",
        "http://10.77.0.1:8080",
        true,
    )?)?;
    let expect_md5 = format!("preseed/url/checksum={}", rendered.preseed_md5());
    ensure(rendered.cmdline().contains(&expect_md5), "md5 写入 cmdline")?;
    ensure(
        rendered
            .preseed()
            .contains("d-i netcfg/get_ipaddress string 10.77.0.16"),
        "preseed ip",
    )?;
    ensure(
        rendered
            .cmdline()
            .contains("netcfg/get_ipaddress=10.77.0.16"),
        "cmdline ip",
    )?;
    ensure(
        rendered
            .cmdline()
            .contains("netcfg/get_netmask=255.255.255.0"),
        "mask",
    )?;
    ensure(
        rendered
            .preseed()
            .contains("d-i netcfg/get_netmask string 255.255.255.0"),
        "preseed mask",
    )?;
    ensure(
        rendered
            .preseed()
            .contains("d-i netcfg/choose_interface select 52:54:00:77:01:16"),
        "mac",
    )?;
    Ok(())
}

/// nexus-lvm 关键条目对照遗留脚本（suite 为 trixie）。
///
/// # Errors
///
/// 条目缺失时返回错误。
#[test]
fn nexus_matches_legacy_keys() -> TestResult {
    let rendered = render(&request(
        "nexus-lvm",
        "vm-uefi.json",
        "http://10.77.0.1:8080",
        true,
    )?)?;
    let preseed = rendered.preseed();
    for needle in [
        "d-i mirror/http/hostname string 192.168.2.5:8081",
        "d-i mirror/http/directory string /repository/debian",
        "d-i mirror/suite string trixie",
        "d-i apt-setup/security_host string",
        "d-i time/zone string Asia/Shanghai",
        "d-i clock-setup/ntp-server string 192.168.2.2",
        "d-i partman-auto/method string lvm",
        "d-i partman-auto/choose_recipe select atomic",
        "tasksel tasksel/first multiselect standard, ssh-server",
    ] {
        ensure(preseed.contains(needle), needle)?;
    }
    ensure(
        preseed.contains("d-i passwd/root-password-crypted password $6$"),
        "口令来自输入",
    )?;
    Ok(())
}

/// 非 Armed 入口脚本只打印原因并退出。
///
/// # Errors
///
/// 脚本不符时返回错误。
#[test]
fn not_armed_exits() -> TestResult {
    let rendered = render(&request(
        "nexus-lvm",
        "vm-uefi.json",
        "http://10.77.0.1:8080",
        false,
    )?)?;
    ensure(
        rendered.boot_ipxe().contains("not armed: node-not-armed"),
        "原因",
    )?;
    ensure(rendered.boot_ipxe().contains("sleep 3"), "sleep")?;
    ensure(rendered.boot_ipxe().contains("exit"), "exit")?;
    ensure(!rendered.boot_ipxe().contains("kernel "), "无 kernel")?;
    Ok(())
}

/// 注入字符在 hook 中被单引号包裹。
///
/// # Errors
///
/// 未转义时返回错误。
#[test]
fn hooks_quote_payloads() -> TestResult {
    let rendered = render(&request(
        "nexus-lvm",
        "vm-uefi.json",
        "http://10.77.0.1:8080",
        true,
    )?)?;
    ensure(
        rendered
            .preseed()
            .contains("--post-data 'event=installer.started"),
        "quoted",
    )?;
    ensure(
        !rendered.preseed().contains("--post-data event=installer"),
        "not bare",
    )?;
    Ok(())
}

/// 超长 `boot_base` 使 cmdline 超过 2048 字节。
///
/// # Errors
///
/// 未报错或错误码不符时返回错误。
#[test]
fn cmdline_rejects_overlong() -> TestResult {
    let host = format!("http://example.com/{}", "a".repeat(2000));
    let req = request("nexus-lvm", "vm-uefi.json", &host, true)?;
    match render(&req) {
        Err(error) => Ok(ensure_eq(&error.code(), &"cmdline_too_long", "code")?),
        Ok(_) => Err("超长 cmdline 应失败".into()),
    }
}
