//! 逐行文本差异（LCS），用于同一计划两个版本的逐模块脚本对比。

use serde::Serialize;

/// 一行差异。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", content = "text", rename_all = "snake_case")]
pub enum DiffLine {
    /// 两边相同。
    Same(String),
    /// 新版本新增。
    Added(String),
    /// 旧版本删除。
    Removed(String),
}

/// 超过该单元数时不做 LCS，直接整段替换。
const MAX_CELLS: usize = 4_000_000;

/// 读取 LCS 表中的值（越界为 0）。
fn cell(table: &[Vec<usize>], row: usize, col: usize) -> usize {
    table
        .get(row)
        .and_then(|line| line.get(col))
        .copied()
        .unwrap_or(0)
}

/// `table[i][j]` = `old[i..]` 与 `new[j..]` 的 LCS 长度。
fn lcs_table(old: &[&str], new: &[&str]) -> Vec<Vec<usize>> {
    let mut table = vec![vec![0_usize; new.len().saturating_add(1)]; old.len().saturating_add(1)];
    for (row, left) in old.iter().enumerate().rev() {
        for (col, right) in new.iter().enumerate().rev() {
            let down = row.saturating_add(1);
            let across = col.saturating_add(1);
            let value = if left == right {
                cell(&table, down, across).saturating_add(1)
            } else {
                cell(&table, down, col).max(cell(&table, row, across))
            };
            if let Some(slot) = table.get_mut(row).and_then(|line| line.get_mut(col)) {
                *slot = value;
            }
        }
    }
    table
}

/// 逐行比较两段文本。
#[must_use]
pub fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let left: Vec<&str> = old.lines().collect();
    let right: Vec<&str> = new.lines().collect();
    if left.len().saturating_mul(right.len()) > MAX_CELLS {
        let removed = left
            .iter()
            .map(|line| DiffLine::Removed((*line).to_owned()));
        let added = right.iter().map(|line| DiffLine::Added((*line).to_owned()));
        return removed.chain(added).collect();
    }
    let table = lcs_table(&left, &right);
    let mut out = Vec::new();
    let (mut row, mut col) = (0_usize, 0_usize);
    loop {
        match (left.get(row), right.get(col)) {
            (Some(old_line), Some(new_line)) if old_line == new_line => {
                out.push(DiffLine::Same((*old_line).to_owned()));
                row = row.saturating_add(1);
                col = col.saturating_add(1);
            }
            (Some(old_line), Some(_))
                if cell(&table, row.saturating_add(1), col)
                    >= cell(&table, row, col.saturating_add(1)) =>
            {
                out.push(DiffLine::Removed((*old_line).to_owned()));
                row = row.saturating_add(1);
            }
            (Some(old_line), None) => {
                out.push(DiffLine::Removed((*old_line).to_owned()));
                row = row.saturating_add(1);
            }
            (_, Some(new_line)) => {
                out.push(DiffLine::Added((*new_line).to_owned()));
                col = col.saturating_add(1);
            }
            (None, None) => break,
        }
    }
    out
}
