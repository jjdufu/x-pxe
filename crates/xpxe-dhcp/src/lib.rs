//! x-pxe 的 `DHCPv4` 编解码与 `ProxyDHCP` 应答构造。
//!
//! - [`wire`]：BOOTP 定长头 + DHCP 选项的解析与序列化（RFC 2131/2132），不依赖第三方 crate。
//! - [`proxy`]：ProxyDHCP（PXE 规范 2.1、UEFI HTTP Boot）的应答报文构造。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

pub mod proxy;
pub mod wire;
