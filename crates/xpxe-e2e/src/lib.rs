//! x-pxe 进程内端到端测试。
//!
//! 用例位于 `tests/` 下的目录形式集成测试，本库承载共享的编排代码：进程内控制面、
//! 极简 HTTP 客户端、模拟镜像、前置数据与 bootnet 回放。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

pub mod bootnet;
pub mod flow;
pub mod harness;
pub mod http;
pub mod mirror;
pub mod seed;

/// 可跨线程传递的测试结果（异步助手的 future 须为 `Send`）。
pub type Outcome<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
