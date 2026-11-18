//! `netinfo` 模块单元测试。

use std::io;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::{iface_ipv4, iface_mtu, parse_kernel, socket_error};

/// 内核版本解析、回环地址与 MTU、错误提示。
///
/// # Errors
///
/// 断言失败。
#[test]
fn interface_facts() -> TestResult {
    ensure_eq(&parse_kernel("6.12.113+"), &Some((6, 12)), "带后缀")?;
    ensure_eq(&parse_kernel("5.4.0-150-generic"), &Some((5, 4)), "发行版")?;
    ensure_eq(&parse_kernel("garbage"), &None, "非法")?;
    ensure(
        !iface_ipv4("lo")?.is_unspecified(),
        "回环网卡可解析出源地址",
    )?;
    ensure(iface_ipv4("no-such-if0").is_err(), "不存在的网卡")?;
    ensure(iface_mtu("lo") >= 1500, "回环 MTU")?;
    ensure_eq(&iface_mtu("no-such-if0"), &1500, "缺省 1500")?;
    let busy = socket_error(
        "br0",
        "绑定 67/udp ",
        &io::Error::from(io::ErrorKind::AddrInUse),
    );
    ensure(busy.message().contains("不会抢占"), "端口占用提示")?;
    Ok(())
}
