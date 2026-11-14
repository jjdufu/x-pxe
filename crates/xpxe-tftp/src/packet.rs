//! TFTP 报文。

/// 读请求。
const OP_RRQ: u16 = 1;
/// 写请求。
const OP_WRQ: u16 = 2;
/// 数据。
const OP_DATA: u16 = 3;
/// 确认。
const OP_ACK: u16 = 4;
/// 错误。
const OP_ERROR: u16 = 5;
/// 选项确认。
const OP_OACK: u16 = 6;

/// TFTP 错误码（RFC 1350 / 2347）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// 未定义。
    Undefined,
    /// 文件不存在。
    FileNotFound,
    /// 访问被拒绝。
    AccessViolation,
    /// 非法操作。
    IllegalOperation,
    /// 未知传输 ID。
    UnknownTransfer,
    /// 选项协商失败。
    OptionRefused,
}

impl ErrorCode {
    /// 线上编码。
    #[must_use]
    pub const fn to_u16(self) -> u16 {
        match self {
            Self::Undefined => 0,
            Self::FileNotFound => 1,
            Self::AccessViolation => 2,
            Self::IllegalOperation => 4,
            Self::UnknownTransfer => 5,
            Self::OptionRefused => 8,
        }
    }

    /// 从线上编码解析；未知值归为 [`ErrorCode::Undefined`]。
    #[must_use]
    pub const fn from_u16(value: u16) -> Self {
        match value {
            1 => Self::FileNotFound,
            2 => Self::AccessViolation,
            4 => Self::IllegalOperation,
            5 => Self::UnknownTransfer,
            8 => Self::OptionRefused,
            _ => Self::Undefined,
        }
    }
}

/// 一个 TFTP 报文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// 读请求：文件名、模式与选项（键已转小写）。
    Read {
        /// 文件名。
        filename: String,
        /// 传输模式（`octet`/`netascii`）。
        mode: String,
        /// RFC 2347 选项。
        options: Vec<(String, String)>,
    },
    /// 写请求（只读服务端一律拒绝）。
    Write,
    /// 数据块。
    Data {
        /// 块号。
        block: u16,
        /// 内容。
        data: Vec<u8>,
    },
    /// 确认。
    Ack {
        /// 块号。
        block: u16,
    },
    /// 错误。
    Error {
        /// 错误码。
        code: ErrorCode,
        /// 说明。
        message: String,
    },
    /// 选项确认。
    OptionAck {
        /// 接受的选项。
        options: Vec<(String, String)>,
    },
}

/// 解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// 报文过短。
    TooShort,
    /// 未知操作码。
    UnknownOpcode(u16),
    /// 字符串缺少 NUL 结尾或不是 UTF-8。
    BadString,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => f.write_str("报文过短"),
            Self::UnknownOpcode(op) => write!(f, "未知操作码 {op}"),
            Self::BadString => f.write_str("字符串字段非法"),
        }
    }
}

impl std::error::Error for ParseError {}

/// 读出的整数与剩余字节。
type Split<'bytes> = (u16, &'bytes [u8]);

/// 读取两个字节的大端整数。
///
/// # Errors
///
/// 不足两个字节时返回 [`ParseError::TooShort`]。
fn read_u16(bytes: &[u8]) -> Result<Split<'_>, ParseError> {
    let (head, rest) = bytes.split_first_chunk::<2>().ok_or(ParseError::TooShort)?;
    Ok((u16::from_be_bytes(*head), rest))
}

/// 按 NUL 切分字符串字段。
///
/// # Errors
///
/// 字段不以 NUL 结尾或不是 UTF-8 时返回 [`ParseError::BadString`]。
fn strings(body: &[u8]) -> Result<Vec<String>, ParseError> {
    let Some(stripped) = body.strip_suffix(&[0]) else {
        return Err(ParseError::BadString);
    };
    stripped
        .split(|byte| *byte == 0)
        .map(|part| String::from_utf8(part.to_vec()).map_err(|_utf8| ParseError::BadString))
        .collect()
}

impl Packet {
    /// 解析一个 UDP 负载。
    ///
    /// # Errors
    ///
    /// 报文结构非法时返回 [`ParseError`]。
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let (opcode, rest) = read_u16(bytes)?;
        match opcode {
            OP_RRQ => {
                let fields = strings(rest)?;
                let mut iter = fields.into_iter();
                let filename = iter.next().ok_or(ParseError::BadString)?;
                let mode = iter
                    .next()
                    .ok_or(ParseError::BadString)?
                    .to_ascii_lowercase();
                let rest_fields: Vec<String> = iter.collect();
                let options = rest_fields
                    .chunks(2)
                    .filter_map(|pair| match pair {
                        [key, value] => Some((key.to_ascii_lowercase(), value.clone())),
                        _ => None,
                    })
                    .collect();
                Ok(Self::Read {
                    filename,
                    mode,
                    options,
                })
            }
            OP_WRQ => Ok(Self::Write),
            OP_DATA => {
                let (block, data) = read_u16(rest)?;
                Ok(Self::Data {
                    block,
                    data: data.to_vec(),
                })
            }
            OP_ACK => {
                let (block, _trailing) = read_u16(rest)?;
                Ok(Self::Ack { block })
            }
            OP_ERROR => {
                let (code, text) = read_u16(rest)?;
                let message = strings(text)?.into_iter().next().unwrap_or_default();
                Ok(Self::Error {
                    code: ErrorCode::from_u16(code),
                    message,
                })
            }
            OP_OACK => {
                let fields = strings(rest)?;
                let options = fields
                    .chunks(2)
                    .filter_map(|pair| match pair {
                        [key, value] => Some((key.clone(), value.clone())),
                        _ => None,
                    })
                    .collect();
                Ok(Self::OptionAck { options })
            }
            other => Err(ParseError::UnknownOpcode(other)),
        }
    }

    /// 序列化。
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let push_str = |buf: &mut Vec<u8>, text: &str| {
            buf.extend_from_slice(text.as_bytes());
            buf.push(0);
        };
        match self {
            Self::Read {
                filename,
                mode,
                options,
            } => {
                out.extend_from_slice(&OP_RRQ.to_be_bytes());
                push_str(&mut out, filename);
                push_str(&mut out, mode);
                for (key, value) in options {
                    push_str(&mut out, key);
                    push_str(&mut out, value);
                }
            }
            Self::Write => out.extend_from_slice(&OP_WRQ.to_be_bytes()),
            Self::Data { block, data } => {
                out.extend_from_slice(&OP_DATA.to_be_bytes());
                out.extend_from_slice(&block.to_be_bytes());
                out.extend_from_slice(data);
            }
            Self::Ack { block } => {
                out.extend_from_slice(&OP_ACK.to_be_bytes());
                out.extend_from_slice(&block.to_be_bytes());
            }
            Self::Error { code, message } => {
                out.extend_from_slice(&OP_ERROR.to_be_bytes());
                out.extend_from_slice(&code.to_u16().to_be_bytes());
                push_str(&mut out, message);
            }
            Self::OptionAck { options } => {
                out.extend_from_slice(&OP_OACK.to_be_bytes());
                for (key, value) in options {
                    push_str(&mut out, key);
                    push_str(&mut out, value);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
