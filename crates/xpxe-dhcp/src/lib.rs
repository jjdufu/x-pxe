//! x-pxe 的 `DHCPv4` 编解码与 `ProxyDHCP` 应答构造。
//!
//! - [`wire`]：BOOTP 定长头 + DHCP 选项的解析与序列化（RFC 2131/2132），不依赖第三方 crate。
//! - [`proxy`]：ProxyDHCP（PXE 规范 2.1、UEFI HTTP Boot）的应答报文构造。
//! - [`answer`]：把领域决策编码为应答报文，提取请求视图，计算目的地址。
//! - [`probe`]：现网 DHCP 探测报文与应答分类。
//! - [`link`]：`DhcpTransport` 抽象、UDP 套接字实现与测试用内存实现。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

pub mod answer;
pub mod link;
pub mod probe;
pub mod proxy;
pub mod wire;
