//! `DHCPv4` 报文的线上格式。
//!
//! 报文 = 236 字节 BOOTP 定长头 + 4 字节魔数 `99.130.83.99` + 选项（`code, len, value`）。
//! 解析只做结构校验，不做语义判断；未知选项原样保留，序列化时按原顺序写回。

use std::net::Ipv4Addr;

/// BOOTP 定长头长度（不含魔数）。
const HEADER_LEN: usize = 236;
/// DHCP 魔数。
const MAGIC: [u8; 4] = [99, 130, 83, 99];
/// 接受的最大报文长度：超过以太网 MTU 的报文直接拒绝。
pub const MAX_PACKET_LEN: usize = 1500;
/// 填充选项。
const OPT_PAD: u8 = 0;
/// 结束选项。
const OPT_END: u8 = 255;

/// 常用选项编号。
pub mod code {
    /// 子网掩码。
    pub const SUBNET_MASK: u8 = 1;
    /// 厂商封装选项（PXE 子选项）。
    pub const VENDOR_SPECIFIC: u8 = 43;
    /// 请求的 IP 地址。
    pub const REQUESTED_IP: u8 = 50;
    /// 报文类型。
    pub const MESSAGE_TYPE: u8 = 53;
    /// 服务器标识。
    pub const SERVER_ID: u8 = 54;
    /// 参数请求列表。
    pub const PARAMETER_LIST: u8 = 55;
    /// 最大报文长度。
    pub const MAX_MESSAGE_SIZE: u8 = 57;
    /// 厂商类别标识（`PXEClient:…`、`HTTPClient:…`）。
    pub const VENDOR_CLASS: u8 = 60;
    /// 客户端标识。
    pub const CLIENT_ID: u8 = 61;
    /// TFTP 服务器名。
    pub const TFTP_SERVER: u8 = 66;
    /// 引导文件名。
    pub const BOOTFILE: u8 = 67;
    /// 用户类别（iPXE 发 `iPXE`）。
    pub const USER_CLASS: u8 = 77;
    /// 客户端系统架构（RFC 4578）。
    pub const CLIENT_ARCH: u8 = 93;
    /// 客户端网络接口标识（UNDI 版本）。
    pub const CLIENT_NDI: u8 = 94;
    /// 客户端机器标识（UUID/GUID）。
    pub const CLIENT_UUID: u8 = 97;
    /// iPXE 封装选项。
    pub const IPXE_ENCAP: u8 = 175;
}

/// 报文方向（BOOTP `op`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// 客户端请求。
    Request,
    /// 服务器应答。
    Reply,
}

/// DHCP 报文类型（选项 53）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// DHCPDISCOVER。
    Discover,
    /// DHCPOFFER。
    Offer,
    /// DHCPREQUEST。
    Request,
    /// DHCPDECLINE。
    Decline,
    /// DHCPACK。
    Ack,
    /// DHCPNAK。
    Nak,
    /// DHCPRELEASE。
    Release,
    /// DHCPINFORM。
    Inform,
    /// 其他取值。
    Other(u8),
}

impl MessageType {
    /// 从选项字节解析。
    #[must_use]
    pub const fn from_byte(value: u8) -> Self {
        match value {
            1 => Self::Discover,
            2 => Self::Offer,
            3 => Self::Request,
            4 => Self::Decline,
            5 => Self::Ack,
            6 => Self::Nak,
            7 => Self::Release,
            8 => Self::Inform,
            other => Self::Other(other),
        }
    }

    /// 选项字节。
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Discover => 1,
            Self::Offer => 2,
            Self::Request => 3,
            Self::Decline => 4,
            Self::Ack => 5,
            Self::Nak => 6,
            Self::Release => 7,
            Self::Inform => 8,
            Self::Other(other) => other,
        }
    }
}

/// 解析失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// 报文短于定长头 + 魔数。
    TooShort,
    /// 报文超过 [`MAX_PACKET_LEN`]。
    TooLong,
    /// `op` 不是 1 或 2。
    BadOp(u8),
    /// 魔数不符。
    BadMagic,
    /// `hlen` 超过 16。
    BadHardwareLen(u8),
    /// 选项长度越界。
    TruncatedOption(u8),
    /// 缺少结束选项。
    MissingEnd,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => f.write_str("报文过短"),
            Self::TooLong => f.write_str("报文超过 1500 字节"),
            Self::BadOp(op) => write!(f, "未知 op {op}"),
            Self::BadMagic => f.write_str("DHCP 魔数不符"),
            Self::BadHardwareLen(len) => write!(f, "硬件地址长度 {len} 超过 16"),
            Self::TruncatedOption(option) => write!(f, "选项 {option} 长度越界"),
            Self::MissingEnd => f.write_str("缺少结束选项"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// 序列化失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// 单个选项值超过 255 字节。
    OptionTooLong(u8),
    /// `sname` 超过 64 字节或 `file` 超过 128 字节。
    FieldTooLong,
    /// 选项编号为 0 或 255（保留给填充与结束）。
    ReservedCode(u8),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OptionTooLong(option) => write!(f, "选项 {option} 超过 255 字节"),
            Self::FieldTooLong => f.write_str("sname/file 字段过长"),
            Self::ReservedCode(option) => write!(f, "选项编号 {option} 保留"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// 一个选项：编号与原始值。
pub type RawOption = (u8, Vec<u8>);

/// 一个 `DHCPv4` 报文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// 方向。
    pub op: Op,
    /// 硬件类型（以太网为 1）。
    pub htype: u8,
    /// 硬件地址长度（以太网为 6）。
    pub hlen: u8,
    /// 中继跳数。
    pub hops: u8,
    /// 事务 ID。
    pub xid: u32,
    /// 客户端已等待的秒数。
    pub secs: u16,
    /// 标志位（最高位为广播标志）。
    pub flags: u16,
    /// 客户端当前地址。
    pub ciaddr: Ipv4Addr,
    /// 分配给客户端的地址。
    pub yiaddr: Ipv4Addr,
    /// 下一跳服务器（TFTP）地址。
    pub siaddr: Ipv4Addr,
    /// 中继代理地址。
    pub giaddr: Ipv4Addr,
    /// 客户端硬件地址（16 字节，前 `hlen` 字节有效）。
    pub chaddr: [u8; 16],
    /// 服务器名（以 NUL 结尾或占满 64 字节）。
    pub sname: Vec<u8>,
    /// 引导文件名（以 NUL 结尾或占满 128 字节）。
    pub file: Vec<u8>,
    /// 选项，保持线上顺序。
    pub options: Vec<RawOption>,
}

/// 按位置顺序读取字节的游标。
#[derive(Debug)]
struct Reader<'buf> {
    /// 尚未读取的字节。
    rest: &'buf [u8],
}

impl<'buf> Reader<'buf> {
    /// 读取 `len` 字节。
    ///
    /// # Errors
    ///
    /// 剩余字节不足时返回 [`DecodeError::TooShort`]。
    fn take(&mut self, len: usize) -> Result<&'buf [u8], DecodeError> {
        let (head, tail) = self
            .rest
            .split_at_checked(len)
            .ok_or(DecodeError::TooShort)?;
        self.rest = tail;
        Ok(head)
    }

    /// 读取定长数组。
    ///
    /// # Errors
    ///
    /// 剩余字节不足时返回 [`DecodeError::TooShort`]。
    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let bytes = self.take(N)?;
        let Ok(array) = <[u8; N]>::try_from(bytes) else {
            return Err(DecodeError::TooShort);
        };
        Ok(array)
    }
}

/// 去掉 `sname`/`file` 末尾的 NUL 填充。
fn trim_nul(field: &[u8]) -> Vec<u8> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    field.get(..end).map_or_else(Vec::new, <[u8]>::to_vec)
}

/// 解析选项区。
///
/// # Errors
///
/// 选项长度越界或没有结束选项时返回错误。
fn decode_options(mut rest: &[u8]) -> Result<Vec<RawOption>, DecodeError> {
    let mut options = Vec::new();
    loop {
        let Some((&option, tail)) = rest.split_first() else {
            return Err(DecodeError::MissingEnd);
        };
        match option {
            OPT_END => return Ok(options),
            OPT_PAD => rest = tail,
            _ => {
                let (&len, body) = tail
                    .split_first()
                    .ok_or(DecodeError::TruncatedOption(option))?;
                let (value, after) = body
                    .split_at_checked(usize::from(len))
                    .ok_or(DecodeError::TruncatedOption(option))?;
                options.push((option, value.to_vec()));
                rest = after;
            }
        }
    }
}

impl Message {
    /// 从 UDP 负载解析报文。
    ///
    /// # Errors
    ///
    /// 结构不合法时返回 [`DecodeError`]。
    pub fn decode(packet: &[u8]) -> Result<Self, DecodeError> {
        if packet.len() > MAX_PACKET_LEN {
            return Err(DecodeError::TooLong);
        }
        let mut reader = Reader { rest: packet };
        let [op_byte, htype, hlen, hops] = reader.array::<4>()?;
        let op = match op_byte {
            1 => Op::Request,
            2 => Op::Reply,
            other => return Err(DecodeError::BadOp(other)),
        };
        if hlen > 16 {
            return Err(DecodeError::BadHardwareLen(hlen));
        }
        let xid = u32::from_be_bytes(reader.array::<4>()?);
        let secs = u16::from_be_bytes(reader.array::<2>()?);
        let flags = u16::from_be_bytes(reader.array::<2>()?);
        let client_addr = Ipv4Addr::from(reader.array::<4>()?);
        let your_addr = Ipv4Addr::from(reader.array::<4>()?);
        let next_server = Ipv4Addr::from(reader.array::<4>()?);
        let relay_agent = Ipv4Addr::from(reader.array::<4>()?);
        let chaddr = reader.array::<16>()?;
        let sname = trim_nul(reader.take(64)?);
        let file = trim_nul(reader.take(128)?);
        if reader.array::<4>()? != MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let options = decode_options(reader.rest)?;
        Ok(Self {
            op,
            htype,
            hlen,
            hops,
            xid,
            secs,
            flags,
            ciaddr: client_addr,
            yiaddr: your_addr,
            siaddr: next_server,
            giaddr: relay_agent,
            chaddr,
            sname,
            file,
            options,
        })
    }

    /// 序列化为 UDP 负载。
    ///
    /// # Errors
    ///
    /// 字段或选项超长、使用保留选项编号时返回 [`EncodeError`]。
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::with_capacity(HEADER_LEN.saturating_add(64));
        out.push(match self.op {
            Op::Request => 1,
            Op::Reply => 2,
        });
        out.extend_from_slice(&[self.htype, self.hlen, self.hops]);
        out.extend_from_slice(&self.xid.to_be_bytes());
        out.extend_from_slice(&self.secs.to_be_bytes());
        out.extend_from_slice(&self.flags.to_be_bytes());
        for addr in [self.ciaddr, self.yiaddr, self.siaddr, self.giaddr] {
            out.extend_from_slice(&addr.octets());
        }
        out.extend_from_slice(&self.chaddr);
        push_fixed(&mut out, &self.sname, 64)?;
        push_fixed(&mut out, &self.file, 128)?;
        out.extend_from_slice(&MAGIC);
        for (option, value) in &self.options {
            if *option == OPT_PAD || *option == OPT_END {
                return Err(EncodeError::ReservedCode(*option));
            }
            let Ok(len) = u8::try_from(value.len()) else {
                return Err(EncodeError::OptionTooLong(*option));
            };
            out.push(*option);
            out.push(len);
            out.extend_from_slice(value);
        }
        out.push(OPT_END);
        Ok(out)
    }

    /// 第一个编号为 `option` 的选项值。
    #[must_use]
    pub fn option(&self, option: u8) -> Option<&[u8]> {
        self.options
            .iter()
            .find(|(found, _)| *found == option)
            .map(|(_, value)| value.as_slice())
    }

    /// 报文类型（选项 53）。
    #[must_use]
    pub fn message_type(&self) -> Option<MessageType> {
        self.option(code::MESSAGE_TYPE)
            .and_then(|value| value.first().copied())
            .map(MessageType::from_byte)
    }

    /// 以太网 MAC（`htype = 1` 且 `hlen = 6` 时）。
    #[must_use]
    pub fn mac(&self) -> Option<[u8; 6]> {
        if self.htype != 1 || self.hlen != 6 {
            return None;
        }
        let bytes = self.chaddr.get(..6)?;
        <[u8; 6]>::try_from(bytes).ok()
    }

    /// 客户端架构（选项 93 的第一个值）。
    #[must_use]
    pub fn client_arch(&self) -> Option<u16> {
        self.option(code::CLIENT_ARCH)
            .and_then(|value| value.get(..2))
            .and_then(|bytes| <[u8; 2]>::try_from(bytes).ok())
            .map(u16::from_be_bytes)
    }

    /// 厂商类别标识（选项 60）的文本。
    #[must_use]
    pub fn vendor_class(&self) -> Option<String> {
        self.option(code::VENDOR_CLASS)
            .map(|value| String::from_utf8_lossy(value).into_owned())
    }

    /// 用户类别（选项 77）。兼容 RFC 3004 的长度前缀格式与 iPXE 的裸字符串格式。
    #[must_use]
    pub fn user_class(&self) -> Option<String> {
        let value = self.option(code::USER_CLASS)?;
        let text = match value.split_first() {
            Some((&len, rest)) if usize::from(len) == rest.len() && !rest.is_empty() => rest,
            Some(_) | None => value,
        };
        Some(String::from_utf8_lossy(text).into_owned())
    }

    /// 是否为 iPXE 客户端：选项 77 为 `iPXE` 或带有选项 175，二者任一。
    #[must_use]
    pub fn is_ipxe(&self) -> bool {
        self.user_class().is_some_and(|class| class == "iPXE")
            || self.option(code::IPXE_ENCAP).is_some()
    }
}

/// 写入定长、NUL 填充的字段。
///
/// # Errors
///
/// 内容超过 `width` 时返回 [`EncodeError::FieldTooLong`]。
fn push_fixed(out: &mut Vec<u8>, field: &[u8], width: usize) -> Result<(), EncodeError> {
    let pad = width
        .checked_sub(field.len())
        .ok_or(EncodeError::FieldTooLong)?;
    out.extend_from_slice(field);
    out.resize(out.len().saturating_add(pad), 0);
    Ok(())
}

#[cfg(test)]
mod tests;
