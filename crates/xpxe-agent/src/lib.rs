//! `xpxe-agent`：目标机首次启动时逐个执行初始化计划中的模块脚本并回报。
//!
//! 生命周期（超时、续跑、回报、收尾）由本 crate 保证；模块内容是渲染出的 bash 脚本。
//! 重启、禁用服务、取当前时间等副作用经 [`Host`] 注入，网络回报经 [`Reporter`] 注入，
//! 测试用记录实现替换。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

mod exec;
mod host;
mod plan;
mod report;
mod run;

pub use exec::{ModuleOutcome, run_script};
pub use host::{Host, SystemHost};
pub use plan::{AgentError, load_plan, verify_plan};
pub use report::{AgentEvent, HttpReporter, Reporter, RetryPolicy, deliver};
pub use run::{AgentConfig, RunOutcome, run};
