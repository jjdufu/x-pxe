//! 片段结构检查。

use xpxe_test_support::{TestResult, ensure};

use super::check_shell;

/// 合法片段通过，bash 专有语法给警告。
///
/// # Errors
///
/// 判断不符时返回错误。
#[test]
fn accepts_valid() -> TestResult {
    let ok = [
        "echo hi\n",
        "if [ -f /x ]; then\n  echo 'a (b'\nelif true; then :; else echo \"$(date)\"; fi\n",
        "case \"$1\" in\n  a|b) echo x ;;\n  *) echo y ;;\nesac\n",
        "for i in 1 2; do\n  echo $((i + 1))\ndone\n",
        "cat > /tmp/x << 'EOF'\nif ( unbalanced\nEOF\necho done\n",
        "f() { echo `uname -r`; }\nf\n",
        "echo a \\\n  b # comment with ' quote\n",
        "x=\"multi\nline\"\necho \"$x\"\n",
    ];
    for body in ok {
        let warnings = check_shell(body).map_err(|error| format!("{body:?}: {error}"))?;
        ensure(warnings.is_empty(), &format!("无警告：{body:?}"))?;
    }
    let warned = check_shell("if [[ -n x ]]; then arr=(1 2); fi\n")?;
    ensure(warned.len() == 2, "两条警告")?;
    Ok(())
}

/// 结构错误被拒绝。
///
/// # Errors
///
/// 非法片段被接受时返回错误。
#[test]
fn rejects_invalid() -> TestResult {
    let bad = [
        "echo 'oops\n",
        "echo \"oops\n",
        "echo `oops\n",
        "if true; then echo x\n",
        "fi\n",
        "for i in 1; do echo\n",
        "cat << EOF\nno end\n",
        "echo $(date\n",
        "echo )\n",
        "{ echo x\n",
        "case x in a) ;;\n",
        "cat <<\n",
    ];
    for body in bad {
        ensure(check_shell(body).is_err(), &format!("应拒绝：{body:?}"))?;
    }
    Ok(())
}
