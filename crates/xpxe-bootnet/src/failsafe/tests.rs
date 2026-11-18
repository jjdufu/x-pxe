//! `failsafe` 模块单元测试。

use std::time::{Duration, Instant};

use xpxe_domain::{BootNetwork, DirectiveSnapshot, HttpUrl};
use xpxe_test_support::{TestResult, ensure};

use super::{Liveness, effective};

/// 30 s 阈值与成功后恢复。
///
/// # Errors
///
/// 断言失败。
#[test]
fn stale_after_thirty_seconds() -> TestResult {
    let start = Instant::now();
    let mut liveness = Liveness::new(start);
    let at = |secs| {
        start
            .checked_add(Duration::from_secs(secs))
            .ok_or("时钟溢出")
    };
    ensure(!liveness.is_stale(at(30)?), "30 s 内不失联")?;
    ensure(liveness.is_stale(at(31)?), "超过 30 s 失联")?;
    liveness.mark_ok(at(31)?);
    ensure(!liveness.is_stale(at(40)?), "成功后恢复")?;
    Ok(())
}

/// 失联时清空指令、保留模式。
///
/// # Errors
///
/// 断言失败。
#[test]
fn effective_clears_directives() -> TestResult {
    let snap =
        DirectiveSnapshot::closed(4, BootNetwork::RedfishOnly, HttpUrl::parse("http://a:1")?);
    let kept = effective(&snap, false);
    ensure(kept == snap, "未失联原样")?;
    let cleared = effective(&snap, true);
    ensure(
        cleared.directives.is_empty() && cleared.rev == 4,
        "失联清空指令",
    )?;
    Ok(())
}
