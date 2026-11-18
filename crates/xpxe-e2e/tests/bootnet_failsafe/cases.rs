//! 停止控制面的内部 API，拨动时钟 30 s 后 bootnet 指令表为空。

use std::time::Duration;

use xpxe_e2e::bootnet::Sim;
use xpxe_e2e::flow::{register_and_arm, run};
use xpxe_e2e::harness::{Harness, Launch};
use xpxe_e2e::{Outcome, seed};
use xpxe_test_support::{ensure, ensure_eq};

/// 失联 30 s 内保留指令，超过后清空。
///
/// # Errors
///
/// 断言失败时返回错误。
#[test]
fn directives_cleared_after_control_loss() -> Outcome {
    run(async {
        let mut harness = Harness::start(Launch::default()).await?;
        let (login, ids) = seed::lan(&harness).await?;
        let spec = seed::lan_spec("e2e-failsafe", "52:54:00:77:01:e2", "uefi");
        let _armed = register_and_arm(&harness, &login, ids, &spec).await?;
        let sim = Sim::new(harness.running()?.internal_addr(), &harness.token_file())?;
        sim.sync().await?;
        ensure_eq(&sim.current().directives.len(), &1, "武装节点出现在指令表")?;
        harness.stop().await;
        ensure(sim.sync().await.is_err(), "控制面停止后拉取失败")?;
        sim.tick_after(Duration::from_secs(20));
        ensure_eq(&sim.current().directives.len(), &1, "20 s 内保留")?;
        sim.tick_after(Duration::from_secs(31));
        ensure(sim.current().directives.is_empty(), "超过 30 s 指令表清空")?;
        Ok(())
    })
}
