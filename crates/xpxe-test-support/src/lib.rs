//! x-pxe 测试支撑库。
//!
//! 门禁禁止在测试里使用 `assert!`、`unwrap` 与 `expect`，测试函数一律返回
//! [`TestResult`]，断言改用 [`ensure`] 与 [`ensure_eq`] 返回错误。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 测试失败时携带的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFailure {
    /// 人类可读的失败原因。
    message: String,
}

impl TestFailure {
    /// 用一条消息构造失败。
    #[must_use]
    pub fn new<M: Into<String>>(message: M) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// 失败原因。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for TestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TestFailure {}

impl From<std::io::Error> for TestFailure {
    fn from(value: std::io::Error) -> Self {
        Self::new(format!("I/O 错误：{value}"))
    }
}

/// 测试函数统一的返回类型。
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// 条件不成立时返回错误，替代 `assert!`。
///
/// # Errors
///
/// `cond` 为假时返回携带 `msg` 的 [`TestFailure`]。
///
/// # Examples
///
/// ```
/// xpxe_test_support::ensure(1_u8 == 1_u8, "相等")?;
/// # Ok::<(), xpxe_test_support::TestFailure>(())
/// ```
pub fn ensure(cond: bool, msg: &str) -> Result<(), TestFailure> {
    if cond {
        Ok(())
    } else {
        Err(TestFailure::new(msg))
    }
}

/// 两个值不相等时返回错误，替代 `assert_eq!`，错误中带两侧的 `Debug` 输出。
///
/// # Errors
///
/// `left != right` 时返回 [`TestFailure`]。
pub fn ensure_eq<T: PartialEq + Debug + ?Sized>(
    left: &T,
    right: &T,
    msg: &str,
) -> Result<(), TestFailure> {
    if left == right {
        Ok(())
    } else {
        Err(TestFailure::new(format!(
            "{msg}\n  左：{left:?}\n  右：{right:?}"
        )))
    }
}

/// 读取调用方 crate 下 `tests/fixtures/<path>` 的夹具文件。
///
/// `manifest_dir` 传 `env!("CARGO_MANIFEST_DIR")`。
///
/// # Errors
///
/// 文件不存在或不可读时返回 I/O 错误。
pub fn fixture_bytes(manifest_dir: &str, path: &str) -> std::io::Result<Vec<u8>> {
    fs::read(
        Path::new(manifest_dir)
            .join("tests")
            .join("fixtures")
            .join(path),
    )
}

/// 进程内唯一的临时数据目录，析构时删除。
#[derive(Debug)]
pub struct TempDataDir {
    /// 目录绝对路径。
    path: PathBuf,
}

/// 同一进程内多个临时目录的序号。
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDataDir {
    /// 在系统临时目录下新建一个空目录。
    ///
    /// # Errors
    ///
    /// 目录创建失败时返回 I/O 错误。
    pub fn new() -> std::io::Result<Self> {
        let seq = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path =
            std::env::temp_dir().join(format!("xpxe-test-{}-{nanos}-{seq}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    /// 目录路径。
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDataDir {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            log_cleanup_failure(&self.path, &error);
        }
    }
}

/// 析构失败时写一行 stderr（门禁禁止 `eprintln!`，因此用显式 `Write`）。
fn log_cleanup_failure(path: &Path, error: &std::io::Error) {
    use std::io::Write as _;
    let mut stderr = std::io::stderr().lock();
    if writeln!(stderr, "清理临时目录 {} 失败：{error}", path.display()).is_err() {
        // stderr 不可写时没有更进一步的报告渠道。
    }
}

#[cfg(test)]
mod tests;
