//! 探针的结构化日志：JSON 行输出到标准错误。

/// 初始化 `tracing` 订阅者；重复调用无副作用。
pub(crate) fn init() {
    let _already = tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .try_init();
}
