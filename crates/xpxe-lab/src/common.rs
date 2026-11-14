//! 子命令共用的参数解析、错误类型、实验网卡守卫与 UDP 套接字构造。

use std::io::Write as _;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use socket2::{Domain, Protocol, Socket, Type};

/// 实验网段 10.77.0.0/24 在 `/proc/net/route` 中的小端十六进制写法。
const LAB_NET_HEX: &str = "00004D0A";
/// /24 掩码的小端十六进制写法。
const LAB_MASK_HEX: &str = "00FFFFFF";

/// 探针错误：一条可读的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LabError(pub(crate) String);

impl std::fmt::Display for LabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LabError {}

impl From<std::io::Error> for LabError {
    fn from(value: std::io::Error) -> Self {
        Self(format!("I/O 错误：{value}"))
    }
}

impl From<xpxe_dhcp::wire::DecodeError> for LabError {
    fn from(value: xpxe_dhcp::wire::DecodeError) -> Self {
        Self(format!("DHCP 解码失败：{value}"))
    }
}

impl From<xpxe_dhcp::wire::EncodeError> for LabError {
    fn from(value: xpxe_dhcp::wire::EncodeError) -> Self {
        Self(format!("DHCP 编码失败：{value}"))
    }
}

impl From<xpxe_image::ImageError> for LabError {
    fn from(value: xpxe_image::ImageError) -> Self {
        Self(format!("镜像生成失败：{value}"))
    }
}

impl From<serde_json::Error> for LabError {
    fn from(value: serde_json::Error) -> Self {
        Self(format!("JSON 错误：{value}"))
    }
}

/// 探针结果。
pub(crate) type LabResult<T = ()> = Result<T, LabError>;

/// 极简命令行参数：`--key value` 与布尔开关 `--flag`。
#[derive(Debug, Clone, Default)]
pub(crate) struct Args {
    /// 原始参数（不含子命令名）。
    items: Vec<String>,
}

impl Args {
    /// 从参数列表构造。
    pub(crate) const fn new(items: Vec<String>) -> Self {
        Self { items }
    }

    /// 取某个键的全部取值。
    pub(crate) fn values(&self, key: &str) -> Vec<String> {
        self.items
            .windows(2)
            .filter_map(|pair| match pair {
                [name, value] if name == key => Some(value.clone()),
                _ => None,
            })
            .collect()
    }

    /// 取某个键的最后一个取值。
    pub(crate) fn value(&self, key: &str) -> Option<String> {
        self.values(key).pop()
    }

    /// 取必填键。
    ///
    /// # Errors
    ///
    /// 缺失时返回 [`LabError`]。
    pub(crate) fn required(&self, key: &str) -> LabResult<String> {
        self.value(key)
            .ok_or_else(|| LabError(format!("缺少参数 {key}")))
    }

    /// 布尔开关是否出现。
    pub(crate) fn flag(&self, key: &str) -> bool {
        self.items.iter().any(|item| item == key)
    }

    /// 解析带默认值的键。
    ///
    /// # Errors
    ///
    /// 取值无法解析时返回 [`LabError`]。
    pub(crate) fn parsed<T: std::str::FromStr>(&self, key: &str, default: T) -> LabResult<T> {
        self.value(key).map_or(Ok(default), |text| {
            text.parse()
                .map_err(|_parse| LabError(format!("参数 {key} 取值非法：{text}")))
        })
    }
}

/// 解析 `aa:bb:cc:dd:ee:ff` 形式的 MAC。
///
/// # Errors
///
/// 格式非法时返回 [`LabError`]。
pub(crate) fn parse_mac(text: &str) -> LabResult<[u8; 6]> {
    let bytes: Vec<u8> = text
        .split([':', '-'])
        .map(|part| u8::from_str_radix(part, 16))
        .collect::<Result<_, _>>()
        .map_err(|parse| LabError(format!("MAC 非法：{text}（{parse}）")))?;
    <[u8; 6]>::try_from(bytes).map_err(|wrong| LabError(format!("MAC 长度非法：{wrong:?}")))
}

/// 把 MAC 格式化为小写冒号形式。
pub(crate) fn format_mac(mac: [u8; 6]) -> String {
    mac.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// `/proc/net/route` 中的小端十六进制地址是否落在 10.77.0.0/24。
fn in_lab_net(hex_le: &str) -> bool {
    u32::from_str_radix(hex_le, 16).is_ok_and(|value| matches!(value.to_le_bytes(), [10, 77, 0, _]))
}

/// 确认网卡属于隔离实验网：该网卡上必须有 10.77.0.0/24 的直连路由，且不能承载经实验网以外网关的默认路由
/// （`ns-probe` 中指向 10.77.0.1 的默认路由用于隔离自检，是允许的）。
///
/// 这是“只在实验网上发 DHCP”的硬性守卫：在任何非实验网卡上调用会直接报错退出。
///
/// # Errors
///
/// 网卡不满足条件或路由表不可读时返回 [`LabError`]。
pub(crate) fn ensure_lab_iface(iface: &str) -> LabResult {
    let table = std::fs::read_to_string("/proc/net/route")?;
    let mut lab_route = false;
    for line in table.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [
            name,
            dest,
            gateway,
            _flags,
            _refcnt,
            _use,
            _metric,
            mask,
            ..,
        ] = fields.as_slice()
        else {
            continue;
        };
        if *name != iface {
            continue;
        }
        if *dest == "00000000" && !in_lab_net(gateway) {
            return Err(LabError(format!(
                "{iface} 承载经实验网以外网关的默认路由，不是隔离实验网卡，拒绝运行"
            )));
        }
        if *dest == LAB_NET_HEX && *mask == LAB_MASK_HEX {
            lab_route = true;
        }
    }
    if lab_route {
        Ok(())
    } else {
        Err(LabError(format!(
            "{iface} 上没有 10.77.0.0/24 直连路由，不是隔离实验网卡，拒绝运行"
        )))
    }
}

/// 建立绑定到指定网卡的 UDP 套接字（允许广播、地址复用）。
///
/// # Errors
///
/// 套接字调用失败时返回 [`LabError`]。
pub(crate) fn bound_udp(iface: &str, ip: Ipv4Addr, port: u16) -> LabResult<tokio::net::UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_broadcast(true)?;
    socket.bind_device(Some(iface.as_bytes()))?;
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(ip, port)).into())?;
    socket.set_nonblocking(true)?;
    Ok(tokio::net::UdpSocket::from_std(socket.into())?)
}

/// 构建多线程 tokio 运行时。
///
/// # Errors
///
/// 运行时创建失败时返回 [`LabError`]。
pub(crate) fn runtime() -> LabResult<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}

/// 向标准输出写一行 JSON 并立即刷新（供 `lab/run.sh` 断言）。
///
/// # Errors
///
/// 序列化或写入失败时返回 [`LabError`]。
pub(crate) fn emit(value: &serde_json::Value) -> LabResult {
    let line = serde_json::to_string(value)?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{line}")?;
    stdout.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests;
