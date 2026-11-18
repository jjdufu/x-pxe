//! `render` 子命令测试。

use xpxe_test_support::{TempDataDir, TestResult, ensure};

use crate::common::Args;

/// 渲染 nexus-lvm × vm-uefi 并写出全部文件。
///
/// # Errors
///
/// 渲染或文件缺失时返回错误。
#[test]
fn writes_all_outputs() -> TestResult {
    let dir = TempDataDir::new()?;
    let node = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lab/nodes/vm-uefi.json");
    let out = dir.path().join("r");
    let args = Args::new(
        ["--preset", "nexus-lvm", "--node", node, "--out"]
            .iter()
            .map(|item| (*item).to_owned())
            .chain(std::iter::once(out.display().to_string()))
            .collect(),
    );
    super::run(&args)?;
    for name in [
        "boot.ipxe",
        "cmdline.txt",
        "preseed.cfg",
        "disk-select.sh",
        "late.sh",
        "agent",
    ] {
        ensure(out.join(name).is_file(), name)?;
    }
    let ipxe = std::fs::read_to_string(out.join("boot.ipxe"))?;
    ensure(ipxe.contains("/p/labboot01"), &ipxe)?;
    Ok(())
}

/// `--set` 覆盖镜像源后仍通过校验并写入 preseed。
///
/// # Errors
///
/// 覆盖未生效时返回错误。
#[test]
fn set_overrides_mirror() -> TestResult {
    let dir = TempDataDir::new()?;
    let node = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../lab/nodes/vm-static.json"
    );
    let out = dir.path().join("r");
    let mirror = r#"/mirror={"scheme":"http","host_port":"10.77.0.1:3142","directory":"/deb.debian.org/debian"}"#;
    let args = Args::new(
        [
            "--preset",
            "nexus-lvm",
            "--node",
            node,
            "--set",
            mirror,
            "--out",
        ]
        .iter()
        .map(|item| (*item).to_owned())
        .chain(std::iter::once(out.display().to_string()))
        .collect(),
    );
    super::run(&args)?;
    let preseed = std::fs::read_to_string(out.join("preseed.cfg"))?;
    ensure(
        preseed.contains("d-i mirror/http/hostname string 10.77.0.1:3142"),
        &preseed,
    )?;
    Ok(())
}
