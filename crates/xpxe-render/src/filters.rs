//! 模板过滤器：shell 单引号转义。

/// 用单引号包裹，并将内部 `'` 转成 `'\''`。
#[must_use]
pub(crate) fn sh_quote(raw: &str) -> String {
    let mut out = String::from("'");
    for ch in raw.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests;
