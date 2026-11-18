//! Profile 预设、约束与非法输入。

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{
    CryptHash, InstallAccounts, Keymap, PartitionScheme, PartitionSpec, Prefer, Profile,
    ProfileParts, RootAccount, SizeSpec,
};

/// 两个内置预设均可加载并通过校验。
///
/// # Errors
///
/// 预设无法加载或名称不符时返回错误。
#[test]
fn presets_load() -> TestResult {
    for name in Profile::preset_names() {
        let profile = Profile::load_preset(name)?;
        ensure_eq(&profile.name().as_str(), name, "名称")?;
        if let Err(errors) = profile.check_constraints() {
            return Err(format!("校验失败：{errors:?}").into());
        }
        let _json = serde_json::to_string(profile.parts())?;
    }
    Ok(())
}

/// nexus-lvm 与现网脚本关键字段一致（suite / 安全源 / 时区 / NTP / LVM / 任务）。
///
/// # Errors
///
/// 字段不符时返回错误。
#[test]
fn nexus_lvm_matches_legacy() -> TestResult {
    let profile = Profile::load_preset("nexus-lvm")?;
    ensure_eq(&profile.locale().as_str(), &"en_US.UTF-8", "locale")?;
    ensure_eq(&profile.keymap().as_str(), &"us", "keymap")?;
    ensure_eq(&profile.timezone().as_str(), &"Asia/Shanghai", "tz")?;
    let ntp: Vec<&str> = profile.ntp().iter().map(super::NtpServer::as_str).collect();
    ensure_eq(ntp.as_slice(), ["192.168.2.2"].as_slice(), "ntp")?;
    ensure_eq(
        &profile.mirror().host_port.as_str(),
        &"192.168.2.5:8081",
        "mirror host",
    )?;
    ensure_eq(
        &profile.mirror().directory.as_str(),
        &"/repository/debian",
        "mirror dir",
    )?;
    ensure(
        matches!(profile.security(), super::SecuritySource::Disabled),
        "无 security",
    )?;
    ensure(
        matches!(profile.partitioning(), PartitionScheme::AtomicLvm),
        "atomic lvm",
    )?;
    let tasks: Vec<&str> = profile
        .tasks()
        .unique()
        .iter()
        .map(|task| task.as_str())
        .collect();
    ensure(tasks.contains(&"ssh-server"), "ssh-server")?;
    ensure(tasks.contains(&"standard"), "standard")?;
    ensure(
        matches!(profile.accounts().root, RootAccount::Hash { .. }),
        "root 来自输入哈希",
    )?;
    Ok(())
}

/// 禁用 root 且无 sudo 主用户时校验失败。
///
/// # Errors
///
/// 未被拒绝时返回错误。
#[test]
fn disabled_root_requires_sudo_user() -> TestResult {
    let mut parts = Profile::load_preset("nexus-lvm")?.parts().clone();
    parts.accounts = InstallAccounts {
        root: RootAccount::Disabled,
        primary_user: None,
    };
    match Profile::new(parts) {
        Err(errors) => {
            ensure(
                errors
                    .iter()
                    .any(|error| error.code == "required_when_root_disabled"),
                "应报 required_when_root_disabled",
            )?;
            Ok(())
        }
        Ok(_) => Err("禁用 root 且无主用户应失败".into()),
    }
}

/// 自定义分区缺少 `/` 时失败；`/boot` 在 LVM 上失败。
///
/// # Errors
///
/// 约束未触发时返回错误。
#[test]
fn custom_partition_constraints() -> TestResult {
    let mut parts = Profile::load_preset("official-mirror")?.parts().clone();
    parts.partitioning = PartitionScheme::Custom {
        partitions: vec![PartitionSpec {
            mount: super::MountPoint::Swap,
            size: SizeSpec::Rest,
            fs: super::FsType::Swap,
            on_lvm: false,
        }],
    };
    match Profile::new(parts.clone()) {
        Err(errors) => ensure(
            errors.iter().any(|error| error.code == "root_required"),
            "应要求 root",
        )?,
        Ok(_) => return Err("缺 / 应失败".into()),
    }
    let root = super::MountPoint::Path {
        path: super::UnixAbsPath::parse("/")?,
    };
    let boot = super::MountPoint::Path {
        path: super::UnixAbsPath::parse("/boot")?,
    };
    parts.partitioning = PartitionScheme::Custom {
        partitions: vec![
            PartitionSpec {
                mount: boot,
                size: SizeSpec::Mib { mib: 512 },
                fs: super::FsType::Ext4,
                on_lvm: true,
            },
            PartitionSpec {
                mount: root,
                size: SizeSpec::Rest,
                fs: super::FsType::Ext4,
                on_lvm: true,
            },
        ],
    };
    match Profile::new(parts) {
        Err(errors) => ensure(
            errors.iter().any(|error| error.code == "boot_not_on_lvm"),
            "应禁止 boot on lvm",
        )?,
        Ok(_) => return Err("/boot 在 LVM 上应失败".into()),
    }
    Ok(())
}

/// 官方安全源在离线模式下给出警告。
///
/// # Errors
///
/// 警告缺失或误报时返回错误。
#[test]
fn official_security_warns_when_offline() -> TestResult {
    let official = Profile::load_preset("official-mirror")?;
    ensure(official.offline_warnings(false).is_empty(), "在线无警告")?;
    let code = official
        .offline_warnings(true)
        .first()
        .ok_or("缺少离线警告")?
        .code
        .as_str()
        .to_owned();
    ensure_eq(code.as_str(), "official_unavailable_offline", "离线警告")?;
    let nexus = Profile::load_preset("nexus-lvm")?;
    ensure(
        nexus.offline_warnings(true).is_empty(),
        "已禁用安全源无警告",
    )?;
    Ok(())
}

/// 键盘布局与哈希拒绝注入字符。
///
/// # Errors
///
/// 非法输入被接受时返回错误。
#[test]
fn rejects_injection_in_keymap_and_hash() -> TestResult {
    for bad in ["us\n", "us;rm", "us$(id)", "us`id`", "us'"] {
        ensure(Keymap::parse(bad).is_err(), bad)?;
    }
    ensure(CryptHash::parse("password").is_err(), "明文")?;
    ensure(CryptHash::parse("$6$short$xx").is_err(), "过短")?;
    let good = Profile::load_preset("nexus-lvm")?;
    let RootAccount::Hash { hash } = &good.accounts().root else {
        return Err("nexus-lvm 应含 root 哈希".into());
    };
    let parsed = CryptHash::parse(hash.as_str())?;
    ensure_eq(&parsed.as_str(), &hash.as_str(), "往返")?;
    Ok(())
}

/// 配置差异对哈希字段只显示“已更改”。
///
/// # Errors
///
/// 差异展示不符时返回错误。
#[test]
fn hash_diff_is_redacted() -> TestResult {
    use crate::versioning::{ChangeValue, canonical_json, diff};
    let left = Profile::load_preset("nexus-lvm")?;
    let mut parts = left.parts().clone();
    if let RootAccount::Hash { hash } = &mut parts.accounts.root {
        *hash = super::CryptSecret::parse(
            "$6$othersaltothersa$iFdzdNjxB/OBPF2XSlT.u.sRRMJraJo0vQhUZ5SC65wTzKtg3D9EK5pe8kURU4jdOBRVVOFFTXkn0lEjKv7Am.",
        )?;
    }
    let right = Profile::new(parts).map_err(|errors| format!("{errors:?}"))?;
    let changes = diff(&canonical_json(&left)?, &canonical_json(&right)?);
    let has_secret = changes.iter().any(|change| {
        matches!(
            (&change.before, &change.after),
            (ChangeValue::SecretChanged, ChangeValue::SecretChanged)
        )
    });
    ensure(has_secret, "哈希变更应显示为已更改")?;
    ensure(
        changes
            .iter()
            .all(|change| !format!("{}", change.after).contains("$6$")),
        "差异不得出现哈希原文",
    )?;
    Ok(())
}

/// 默认选盘规则为 `Unique` 且最小 16 GiB。
///
/// # Errors
///
/// 默认值不符时返回错误。
#[test]
fn default_disk_rule() -> TestResult {
    let profile = Profile::load_preset("nexus-lvm")?;
    ensure(matches!(profile.disk().prefer, Prefer::Unique), "unique")?;
    ensure_eq(&profile.disk().size_gib.min, &16, "min")?;
    ensure(
        profile
            .disk()
            .exclude_models
            .iter()
            .any(|model| model.as_str() == "Virtual"),
        "排除 Virtual",
    )?;
    Ok(())
}

/// [`crate::versioning::ConfigDocument`] 元数据。
///
/// # Errors
///
/// KIND/SCHEMA 不符时返回错误。
#[test]
fn document_kind() -> TestResult {
    ensure_eq(
        &<Profile as crate::versioning::ConfigDocument>::KIND.as_str(),
        &"profile",
        "kind",
    )?;
    ensure_eq(
        &<Profile as crate::versioning::ConfigDocument>::SCHEMA,
        &1,
        "schema",
    )?;
    Ok(())
}

/// 主机端口与路径拒绝换行和注入。
///
/// # Errors
///
/// 非法输入被接受时返回错误。
#[test]
fn host_and_path_reject_injection() -> TestResult {
    use super::{HostPort, UnixAbsPath};
    ensure(HostPort::parse("host/../x").is_err(), "slash")?;
    ensure(HostPort::parse("host;rm").is_err(), "semi")?;
    ensure(HostPort::parse("192.168.2.5:8081").is_ok(), "ok host")?;
    ensure(UnixAbsPath::parse("/debian").is_ok(), "ok path")?;
    ensure(UnixAbsPath::parse("/../etc").is_err(), "dotdot")?;
    ensure(UnixAbsPath::parse("relative").is_err(), "relative")?;
    ensure(UnixAbsPath::parse("/foo\nbar").is_err(), "newline")?;
    Ok(())
}

/// 未使用的辅助，避免部分字段在测试中完全未被提到。
///
/// # Errors
///
/// 结构不完整时返回错误。
#[test]
fn profile_parts_roundtrip_fields() -> TestResult {
    let profile = Profile::load_preset("official-mirror")?;
    ensure(profile.firmware_cpio(), "firmware")?;
    ensure(
        profile
            .console()
            .items()
            .iter()
            .any(|item| item.as_str() == "tty0"),
        "tty0",
    )?;
    ensure(profile.suites().updates, "updates")?;
    ensure(!profile.suites().backports, "no backports")?;
    let _unused: ProfileParts = profile.parts().clone();
    Ok(())
}
