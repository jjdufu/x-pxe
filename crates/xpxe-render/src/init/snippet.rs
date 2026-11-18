//! 自定义片段的结构校验：引号、命令替换、here-doc 与 `if/case/do/{`/`(` 配对。
//!
//! 这是一个保守的 POSIX shell 结构检查器，不是完整解析器：它拒绝未闭合的引号、
//! here-doc 与关键字配对错误，并对 bash 专有语法给出警告（目标机用 bash 执行）。

use std::iter::Peekable;
use std::str::Chars;

/// 上下文栈帧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// `'…'`。
    Single,
    /// `"…"`。
    Double,
    /// `` `…` ``。
    Backtick,
    /// `$(…)`。
    CmdSub,
    /// `$((…))`。
    Arith,
    /// `(…)` 子 shell。
    Paren,
    /// `{ …; }`。
    Brace,
    /// `if … fi`。
    If,
    /// `case … esac`。
    Case,
    /// `do … done`。
    Do,
}

impl Frame {
    /// 未闭合时的说明。
    const fn describe(self) -> &'static str {
        match self {
            Self::Single => "单引号未闭合",
            Self::Double => "双引号未闭合",
            Self::Backtick => "反引号未闭合",
            Self::CmdSub => "$( 未闭合",
            Self::Arith => "$(( 未闭合",
            Self::Paren => "( 未闭合",
            Self::Brace => "{ 缺少 }",
            Self::If => "if 缺少 fi",
            Self::Case => "case 缺少 esac",
            Self::Do => "do 缺少 done",
        }
    }
}

/// 待读取的 here-doc。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Heredoc {
    /// 结束标记。
    delimiter: String,
    /// `<<-`：忽略行首制表符。
    strip_tabs: bool,
}

/// 词法状态。
#[derive(Debug, Default)]
struct Lexer {
    /// 上下文栈。
    stack: Vec<Frame>,
    /// 当前词。
    word: String,
    /// 当前词是否从命令位置开始。
    word_at_cmd: bool,
    /// 下一个词是否在命令位置。
    cmd_pos: bool,
    /// 下一个词是 here-doc 结束标记（值为是否 `<<-`）。
    expect_delim: Option<bool>,
    /// 本行登记的 here-doc。
    pending: Vec<Heredoc>,
    /// 正在读取正文的 here-doc。
    body: Vec<Heredoc>,
    /// 第一个错误。
    error: Option<String>,
}

/// 校验结果：通过时返回 bash 专有语法警告。
///
/// # Errors
///
/// 结构错误时返回说明。
pub(crate) fn check_shell(body: &str) -> Result<Vec<String>, String> {
    let mut lexer = Lexer {
        cmd_pos: true,
        ..Lexer::default()
    };
    for (index, line) in body.lines().enumerate() {
        lexer.feed_line(line);
        if let Some(error) = lexer.error.take() {
            return Err(format!("第 {} 行：{error}", index.saturating_add(1)));
        }
    }
    lexer.finish()?;
    Ok(bashisms(body))
}

/// bash 专有语法的启发式警告。
fn bashisms(body: &str) -> Vec<String> {
    [
        ("[[", "使用了 [[ ]]"),
        ("function ", "使用了 function 关键字"),
        ("<<<", "使用了 here-string <<<"),
        ("$'", "使用了 $'…' 字符串"),
        ("=(", "使用了数组赋值"),
        ("${!", "使用了间接展开 ${!…}"),
        ("source ", "使用了 source（POSIX 为 .）"),
    ]
    .into_iter()
    .filter(|(needle, _)| body.contains(needle))
    .map(|(_, message)| format!("bash 专有语法：{message}"))
    .collect()
}

impl Lexer {
    /// 处理一行（不含换行符）。
    fn feed_line(&mut self, line: &str) {
        if let Some(doc) = self.body.first() {
            let text = if doc.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if text == doc.delimiter {
                let _done = self.body.remove(0);
            }
            return;
        }
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            self.step(ch, &mut chars);
            if self.error.is_some() {
                return;
            }
        }
        self.end_of_line();
    }

    /// 处理一个字符。
    fn step(&mut self, ch: char, rest: &mut Peekable<Chars<'_>>) {
        let quoted = matches!(
            self.stack.last(),
            Some(Frame::Single | Frame::Double | Frame::Backtick)
        );
        if quoted && !self.word.is_empty() {
            self.word.push(ch);
        }
        match self.stack.last().copied() {
            Some(Frame::Single) => {
                if ch == '\'' {
                    let _closed = self.stack.pop();
                }
            }
            Some(Frame::Double) => self.double_char(ch, rest),
            Some(Frame::Backtick) => self.backtick_char(ch, rest),
            Some(
                Frame::CmdSub
                | Frame::Arith
                | Frame::Paren
                | Frame::Brace
                | Frame::If
                | Frame::Case
                | Frame::Do,
            )
            | None => self.code_char(ch, rest),
        }
    }

    /// 双引号内。
    fn double_char(&mut self, ch: char, rest: &mut Peekable<Chars<'_>>) {
        match ch {
            '\\' => {
                let _escaped = rest.next();
            }
            '"' => {
                let _closed = self.stack.pop();
            }
            '`' => self.stack.push(Frame::Backtick),
            '$' => self.dollar(rest),
            _ => {}
        }
    }

    /// 反引号内。
    fn backtick_char(&mut self, ch: char, rest: &mut Peekable<Chars<'_>>) {
        match ch {
            '\\' => {
                let _escaped = rest.next();
            }
            '`' => {
                let _closed = self.stack.pop();
            }
            _ => {}
        }
    }

    /// `$` 之后：`$(` 或 `$((`。
    fn dollar(&mut self, rest: &mut Peekable<Chars<'_>>) {
        if rest.peek() == Some(&'(') {
            let _open = rest.next();
            if rest.peek() == Some(&'(') {
                let _open2 = rest.next();
                self.stack.push(Frame::Arith);
            } else {
                self.stack.push(Frame::CmdSub);
                self.cmd_pos = true;
            }
        }
    }

    /// 代码上下文。
    fn code_char(&mut self, ch: char, rest: &mut Peekable<Chars<'_>>) {
        match ch {
            '#' if self.word.is_empty() => while rest.next().is_some() {},
            '\'' => self.word_push(ch, Some(Frame::Single)),
            '"' => self.word_push(ch, Some(Frame::Double)),
            '`' => self.word_push(ch, Some(Frame::Backtick)),
            '\\' => {
                self.word_push(ch, None);
                if let Some(next) = rest.next() {
                    self.word.push(next);
                }
            }
            '$' => {
                self.word_push(ch, None);
                self.dollar(rest);
            }
            ' ' | '\t' => self.finish_word(false),
            ';' | '&' | '|' => self.finish_word(true),
            '<' | '>' => self.redirect(ch, rest),
            '(' | ')' => self.paren(ch, rest),
            _ => self.word_push(ch, None),
        }
    }

    /// 往当前词追加字符，可选地进入引号上下文。
    fn word_push(&mut self, ch: char, frame: Option<Frame>) {
        if self.word.is_empty() {
            self.word_at_cmd = self.cmd_pos;
        }
        self.word.push(ch);
        if let Some(context) = frame {
            self.stack.push(context);
        }
    }

    /// 重定向与 here-doc。
    fn redirect(&mut self, ch: char, rest: &mut Peekable<Chars<'_>>) {
        self.finish_word(false);
        if ch == '<' && rest.peek() == Some(&'<') {
            let _second = rest.next();
            if rest.peek() == Some(&'<') {
                let _third = rest.next();
                return;
            }
            let strip = rest.peek() == Some(&'-');
            if strip {
                let _dash = rest.next();
            }
            self.expect_delim = Some(strip);
        }
    }

    /// 括号。
    fn paren(&mut self, ch: char, rest: &mut Peekable<Chars<'_>>) {
        self.finish_word(true);
        if ch == '(' {
            self.stack.push(Frame::Paren);
            return;
        }
        match self.stack.last().copied() {
            Some(Frame::Paren | Frame::CmdSub) => {
                let _closed = self.stack.pop();
            }
            Some(Frame::Arith) => {
                if rest.peek() == Some(&')') {
                    let _second = rest.next();
                }
                let _closed = self.stack.pop();
            }
            Some(Frame::Case) => {}
            Some(
                Frame::Single
                | Frame::Double
                | Frame::Backtick
                | Frame::Brace
                | Frame::If
                | Frame::Do,
            )
            | None => self.error = Some("多余的 )".to_owned()),
        }
    }

    /// 结束当前词；`separator` 为真时下一个词处于命令位置。
    fn finish_word(&mut self, separator: bool) {
        let word = std::mem::take(&mut self.word);
        if !word.is_empty() {
            if let Some(strip) = self.expect_delim.take() {
                let delimiter: String = word
                    .chars()
                    .filter(|ch| !matches!(ch, '\'' | '"' | '\\'))
                    .collect();
                self.pending.push(Heredoc {
                    delimiter,
                    strip_tabs: strip,
                });
                self.cmd_pos = false;
            } else if self.word_at_cmd {
                self.cmd_pos = self.keyword(&word);
            } else {
                self.cmd_pos = false;
            }
        }
        if separator {
            self.cmd_pos = true;
        }
    }

    /// 处理命令位置上的词；返回下一个词是否仍在命令位置。
    fn keyword(&mut self, word: &str) -> bool {
        match word {
            "if" => self.stack.push(Frame::If),
            "case" => self.stack.push(Frame::Case),
            "do" => self.stack.push(Frame::Do),
            "{" => self.stack.push(Frame::Brace),
            "fi" => self.close(Frame::If, "fi"),
            "esac" => self.close(Frame::Case, "esac"),
            "done" => self.close(Frame::Do, "done"),
            "}" => self.close(Frame::Brace, "}"),
            "then" | "else" | "elif" | "while" | "until" | "!" => {}
            _ => return false,
        }
        true
    }

    /// 弹出期望的帧，否则记错误。
    fn close(&mut self, want: Frame, word: &str) {
        if self.stack.last() == Some(&want) {
            let _closed = self.stack.pop();
        } else {
            self.error = Some(format!("意外的 {word}"));
        }
    }

    /// 行尾。
    fn end_of_line(&mut self) {
        let quoted = matches!(
            self.stack.last(),
            Some(Frame::Single | Frame::Double | Frame::Backtick)
        );
        if quoted {
            return;
        }
        if self.word.ends_with('\\') {
            let _continuation = self.word.pop();
            return;
        }
        self.finish_word(true);
        if self.expect_delim.take().is_some() {
            self.error = Some("<< 后缺少结束标记".to_owned());
        }
        self.body.append(&mut self.pending);
    }

    /// 输入结束时的检查。
    ///
    /// # Errors
    ///
    /// 有未闭合结构时返回说明。
    fn finish(&self) -> Result<(), String> {
        if let Some(doc) = self.body.first() {
            return Err(format!("here-doc 缺少结束标记 {}", doc.delimiter));
        }
        self.stack
            .last()
            .map_or(Ok(()), |frame| Err(frame.describe().to_owned()))
    }
}

#[cfg(test)]
mod tests;
