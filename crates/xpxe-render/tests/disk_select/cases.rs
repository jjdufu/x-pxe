//! 在伪造 sysfs 树上执行选盘脚本（`sh -n` 语法检查 + 实际运行）。

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use xpxe_domain::{HttpUrl, NodeSpec, Profile};
use xpxe_render::{
    ArmedState, RenderRequest, RenderRequestParts, RequestDigests, RequestTokens, render,
};
use xpxe_test_support::{TempDataDir, TestResult, ensure, ensure_eq};

/// 空文件 SHA-256。
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// 20 GiB 的 512 字节扇区数。
const SECTORS_20G: &str = "41943040";
/// 1 GiB 的扇区数。
const SECTORS_1G: &str = "2097152";

/// 一块伪造磁盘。
struct FakeDisk<'spec> {
    /// 内核名。
    name: &'spec str,
    /// 扇区数。
    sectors: &'spec str,
    /// `removable` 值。
    removable: &'spec str,
    /// 型号。
    model: &'spec str,
    /// 序列号。
    serial: &'spec str,
    /// `devices/` 下的总线路径。
    bus: &'spec str,
}

/// 测试沙箱：临时目录、脚本路径、sysfs 根、by-id 根。
struct Sandbox {
    /// 临时目录（析构时删除）。
    _dir: TempDataDir,
    /// 选盘脚本。
    script: PathBuf,
    /// 伪造 sysfs 根。
    sysfs: PathBuf,
    /// 伪造 `/dev/disk/by-id`。
    by_id: PathBuf,
}

/// 用给定选盘规则（JSON）渲染脚本并建立沙箱。
///
/// # Errors
///
/// 渲染或写文件失败时返回错误。
fn sandbox(disk_rule: Option<serde_json::Value>) -> TestResult<Sandbox> {
    let dir = TempDataDir::new()?;
    let mut doc: serde_json::Value = serde_json::to_value(Profile::load_preset("nexus-lvm")?)?;
    if let (Some(rule), Some(obj)) = (disk_rule, doc.as_object_mut()) {
        let _previous = obj.insert("disk".to_owned(), rule);
    }
    let profile: Profile = serde_json::from_value(doc)?;
    let node: NodeSpec = serde_json::from_str(&fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lab/nodes/vm-uefi.json"),
    )?)?;
    let request = RenderRequest::from_parts(RenderRequestParts {
        profile,
        node,
        boot_base: HttpUrl::parse("http://10.77.0.1:8080")?,
        tokens: RequestTokens {
            boot_token: "boot0001".parse()?,
            agent_token: "agent001".parse()?,
            session_short: "sess01".to_owned(),
            event_id: "ev-00000001".parse()?,
        },
        digests: RequestDigests {
            agent_sha256: EMPTY_SHA.to_owned(),
            plan_sha256: EMPTY_SHA.to_owned(),
        },
        armed: ArmedState::armed(),
    });
    let rendered = render(&request)?;
    let script = dir.path().join("disk-select.sh");
    fs::write(&script, rendered.disk_select())?;
    let sysfs = dir.path().join("sys");
    let by_id = dir.path().join("by-id");
    fs::create_dir_all(sysfs.join("block"))?;
    fs::create_dir_all(&by_id)?;
    Ok(Sandbox {
        _dir: dir,
        script,
        sysfs,
        by_id,
    })
}

impl Sandbox {
    /// 按真实 sysfs 布局建盘：属性放在 `devices/<bus>/block/<name>`，`block/<name>` 为相对链接。
    ///
    /// # Errors
    ///
    /// 写文件失败时返回错误。
    fn add(&self, disk: &FakeDisk<'_>) -> TestResult {
        let real = self
            .sysfs
            .join("devices")
            .join(disk.bus)
            .join("block")
            .join(disk.name);
        fs::create_dir_all(real.join("device"))?;
        fs::write(real.join("size"), disk.sectors)?;
        fs::write(real.join("removable"), disk.removable)?;
        fs::write(real.join("ro"), "0")?;
        fs::write(real.join("device/vendor"), "ATA")?;
        fs::write(real.join("device/model"), disk.model)?;
        fs::write(real.join("serial"), disk.serial)?;
        let target = Path::new("../devices")
            .join(disk.bus)
            .join("block")
            .join(disk.name);
        symlink(target, self.sysfs.join("block").join(disk.name))?;
        Ok(())
    }

    /// 建 by-id 链接。
    ///
    /// # Errors
    ///
    /// 建链接失败时返回错误。
    fn by_id_link(&self, id: &str, name: &str) -> TestResult {
        symlink(format!("../../{name}"), self.by_id.join(id))?;
        Ok(())
    }

    /// 运行脚本，返回退出码与标准输出。
    ///
    /// # Errors
    ///
    /// 语法检查失败或无法启动时返回错误。
    fn run(&self) -> TestResult<(i32, String)> {
        let syntax = Command::new("/bin/sh")
            .arg("-n")
            .arg(&self.script)
            .output()?;
        ensure(syntax.status.success(), "sh -n 语法检查")?;
        let output = Command::new("/bin/sh")
            .arg(&self.script)
            .env("XPXE_SYSFS", &self.sysfs)
            .env("XPXE_BY_ID", &self.by_id)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok((output.status.code().unwrap_or(-1_i32), stdout))
    }
}

/// 20 GiB 的 `NVMe` 盘。
const NVME: FakeDisk<'static> = FakeDisk {
    name: "nvme0n1",
    sectors: SECTORS_20G,
    removable: "0",
    model: "SAMSUNG MZ",
    serial: "NVME0001",
    bus: "pci0000:00/0000:00:04.0/nvme/nvme0",
};

/// 1 GiB BMC 虚拟 U 盘。
const BMC_USB: FakeDisk<'static> = FakeDisk {
    name: "sdb",
    sectors: SECTORS_1G,
    removable: "1",
    model: "Virtual Floppy",
    serial: "VUSB0001",
    bus: "pci0000:00/0000:00:14.0/usb1/1-1/host3/target3:0:0/3:0:0:0",
};

/// 20 GiB SATA。
const fn sata(name: &'static str, serial: &'static str) -> FakeDisk<'static> {
    FakeDisk {
        name,
        sectors: SECTORS_20G,
        removable: "0",
        model: "WDC WD20",
        serial,
        bus: "pci0000:00/0000:00:1f.2/ata1/host0/target0:0:0/0:0:0:0",
    }
}

/// 单块 `NVMe` 应入选。
///
/// # Errors
///
/// 未选中时返回错误。
#[test]
fn single_nvme() -> TestResult {
    let sb = sandbox(None)?;
    sb.add(&NVME)?;
    let (code, out) = sb.run()?;
    ensure_eq(&code, &0_i32, "exit")?;
    ensure(out.contains("SELECTED=/dev/nvme0n1"), &out)?;
    ensure(out.contains("nvme0n1|41943040|20|nvme|"), &out)?;
    Ok(())
}

/// `NVMe` + BMC 虚拟 U 盘，应选 `NVMe` 并在清单中写明排除原因。
///
/// # Errors
///
/// 选错盘时返回错误。
#[test]
fn nvme_plus_bmc_usb() -> TestResult {
    let sb = sandbox(None)?;
    sb.add(&NVME)?;
    sb.add(&BMC_USB)?;
    let (code, out) = sb.run()?;
    ensure_eq(&code, &0_i32, "exit")?;
    ensure(out.contains("SELECTED=/dev/nvme0n1"), &out)?;
    ensure(
        out.contains("sdb|2097152|1|usb|ATA|Virtual Floppy|VUSB0001|removable"),
        &out,
    )?;
    Ok(())
}

/// 两块同型号 SATA 在 Unique 下失败。
///
/// # Errors
///
/// 未失败时返回错误。
#[test]
fn two_sata_unique_fails() -> TestResult {
    let sb = sandbox(None)?;
    sb.add(&sata("sda", "WD-111"))?;
    sb.add(&sata("sdc", "WD-222"))?;
    let (code, out) = sb.run()?;
    ensure(code != 0_i32, "应失败")?;
    ensure(out.contains("ERROR=disk_selection"), &out)?;
    ensure(
        out.contains("sda|41943040|20|sata|ATA|WDC WD20|WD-111|ok"),
        &out,
    )?;
    Ok(())
}

/// 全部小于 16 GiB 时失败。
///
/// # Errors
///
/// 未失败时返回错误。
#[test]
fn all_too_small() -> TestResult {
    let sb = sandbox(None)?;
    sb.add(&FakeDisk {
        sectors: SECTORS_1G,
        ..sata("sda", "WD-SMALL")
    })?;
    let (code, out) = sb.run()?;
    ensure(code != 0_i32, "应失败")?;
    ensure(out.contains("ERROR=disk_selection"), &out)?;
    ensure(out.contains("|too_small"), &out)?;
    Ok(())
}

/// by-id 精确匹配：两块 SATA 中只选中被点名的那块。
///
/// # Errors
///
/// 选错盘时返回错误。
#[test]
fn by_id_exact() -> TestResult {
    let rule = serde_json::json!({
        "include": [{ "type": "by_id", "glob": "ata-WDC_WD20_WD-222" }],
        "size_gib": { "min": 16_u32 },
        "prefer": "unique"
    });
    let sb = sandbox(Some(rule))?;
    sb.add(&sata("sda", "WD-111"))?;
    sb.add(&sata("sdc", "WD-222"))?;
    sb.by_id_link("ata-WDC_WD20_WD-111", "sda")?;
    sb.by_id_link("ata-WDC_WD20_WD-222", "sdc")?;
    let (code, out) = sb.run()?;
    ensure_eq(&code, &0_i32, &out)?;
    ensure(out.contains("SELECTED=/dev/sdc"), &out)?;
    ensure(
        out.contains("sda|41943040|20|sata|ATA|WDC WD20|WD-111|include"),
        &out,
    )?;
    Ok(())
}

/// Largest 偏好在多块候选中选最大。
///
/// # Errors
///
/// 选错盘时返回错误。
#[test]
fn prefer_largest() -> TestResult {
    let rule = serde_json::json!({ "size_gib": { "min": 16_u32 }, "prefer": "largest" });
    let sb = sandbox(Some(rule))?;
    sb.add(&sata("sda", "WD-111"))?;
    sb.add(&FakeDisk {
        sectors: "83886080",
        ..sata("sdc", "WD-222")
    })?;
    let (code, out) = sb.run()?;
    ensure_eq(&code, &0_i32, &out)?;
    ensure(out.contains("SELECTED=/dev/sdc"), &out)?;
    Ok(())
}
