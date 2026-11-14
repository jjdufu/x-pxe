//! x-pxe 的只读 TFTP 服务端。
//!
//! - [`packet`]：RFC 1350 报文与 RFC 2347 选项的编解码。
//! - [`session`]：单次读传输的协议状态机（`blksize` RFC 2348、`tsize`/`timeout` RFC 2349、
//!   `windowsize` RFC 7440），纯逻辑、不做 I/O。
//! - [`server`]：基于 tokio 的传输层，每个读请求使用独立的临时端口。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

pub mod packet;
pub mod server;
pub mod session;
