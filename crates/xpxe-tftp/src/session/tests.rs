//! 状态机测试。

use std::sync::Arc;

use xpxe_test_support::{TestFailure, TestResult, ensure, ensure_eq};

use super::{Action, Limits, Session};
use crate::packet::Packet;

/// 构造选项列表。
fn opts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// 取出发送指令中的块号。
///
/// # Errors
///
/// 指令不是发送数据块时返回错误。
fn blocks(action: &Action) -> Result<Vec<u16>, TestFailure> {
    let Action::Send(packets) = action else {
        return Err(TestFailure::new(format!("期望发送，实际 {action:?}")));
    };
    packets
        .iter()
        .map(|packet| {
            if let Packet::Data { block, .. } = packet {
                Ok(*block)
            } else {
                Err(TestFailure::new(format!("期望数据块，实际 {packet:?}")))
            }
        })
        .collect()
}

/// 无选项：512 字节块、最后一块为空时也要发送。
///
/// # Errors
///
/// 行为不符时返回错误。
#[test]
fn plain_transfer_sends_empty_tail() -> TestResult {
    let data: Arc<[u8]> = Arc::from(vec![9_u8; 1024]);
    let mut session = Session::new(data, &[], Limits::default());
    ensure_eq(&session.total_blocks(), &3, "块数")?;
    ensure_eq(&blocks(&session.start())?, &vec![1], "首块")?;
    ensure_eq(
        &blocks(&session.on_packet(&Packet::Ack { block: 1 }))?,
        &vec![2],
        "第二块",
    )?;
    let third = session.on_packet(&Packet::Ack { block: 2 });
    let Action::Send(packets) = &third else {
        return Err(Box::new(TestFailure::new("期望发送空尾块")));
    };
    ensure_eq(
        packets,
        &vec![Packet::Data {
            block: 3,
            data: Vec::new(),
        }],
        "空尾块",
    )?;
    ensure_eq(
        &session.on_packet(&Packet::Ack { block: 3 }),
        &Action::Done,
        "完成",
    )?;
    Ok(())
}

/// 选项协商：先 OACK，ACK 0 后按窗口发送，超出上限的值被截断。
///
/// # Errors
///
/// 协商结果不符时返回错误。
#[test]
fn negotiates_options_and_window() -> TestResult {
    let data: Arc<[u8]> = Arc::from(vec![1_u8; 10_000]);
    let options = opts(&[
        ("blksize", "65464"),
        ("tsize", "0"),
        ("windowsize", "4"),
        ("timeout", "abc"),
    ]);
    let mut session = Session::new(data, &options, Limits::default());
    ensure_eq(&session.blksize(), &1468, "块大小截断")?;
    let Action::Send(first) = session.start() else {
        return Err(Box::new(TestFailure::new("期望 OACK")));
    };
    ensure_eq(
        &first,
        &vec![Packet::OptionAck {
            options: opts(&[("blksize", "1468"), ("tsize", "10000"), ("windowsize", "4")]),
        }],
        "OACK 内容（非法 timeout 被忽略）",
    )?;
    ensure_eq(
        &session.on_packet(&Packet::Ack { block: 5 }),
        &Action::Wait,
        "OACK 前的杂 ACK",
    )?;
    ensure_eq(
        &blocks(&session.on_packet(&Packet::Ack { block: 0 }))?,
        &vec![1, 2, 3, 4],
        "第一窗口",
    )?;
    ensure_eq(
        &blocks(&session.on_packet(&Packet::Ack { block: 2 }))?,
        &vec![3, 4, 5, 6],
        "部分确认后滑动",
    )?;
    ensure_eq(
        &blocks(&session.on_packet(&Packet::Ack { block: 6 }))?,
        &vec![7],
        "最后一块",
    )?;
    ensure_eq(
        &session.on_packet(&Packet::Ack { block: 6 }),
        &Action::Wait,
        "重复 ACK",
    )?;
    ensure_eq(
        &session.on_packet(&Packet::Ack { block: 7 }),
        &Action::Done,
        "完成",
    )?;
    Ok(())
}

/// 超时重传与放弃。
///
/// # Errors
///
/// 行为不符时返回错误。
#[test]
fn retransmits_then_aborts() -> TestResult {
    let data: Arc<[u8]> = Arc::from(vec![0_u8; 100]);
    let limits = Limits {
        retries: 2,
        ..Limits::default()
    };
    let mut session = Session::new(data, &[], limits);
    ensure_eq(&blocks(&session.start())?, &vec![1], "首块")?;
    ensure_eq(&blocks(&session.on_timeout())?, &vec![1], "重传一")?;
    ensure_eq(&blocks(&session.on_timeout())?, &vec![1], "重传二")?;
    ensure_eq(&session.on_timeout(), &Action::Abort(None), "放弃")?;
    Ok(())
}

/// 超过 65535 块时线上块号回绕到 0。
///
/// # Errors
///
/// 回绕行为不符时返回错误。
#[test]
fn block_numbers_roll_over() -> TestResult {
    let data: Arc<[u8]> = Arc::from(vec![0_u8; 8 * 65_540]);
    let options = opts(&[("blksize", "8"), ("windowsize", "64")]);
    let mut session = Session::new(data, &options, Limits::default());
    let _oack = session.start();
    let mut action = session.on_packet(&Packet::Ack { block: 0 });
    let mut seen_zero = false;
    let mut rounds = 0_u32;
    loop {
        let sent = blocks(&action)?;
        seen_zero |= sent.contains(&0);
        let Some(last) = sent.last().copied() else {
            return Err(Box::new(TestFailure::new("空窗口")));
        };
        action = session.on_packet(&Packet::Ack { block: last });
        rounds = rounds.saturating_add(1);
        if action == Action::Done {
            break;
        }
        ensure(rounds < 5000, "未在预期轮数内完成")?;
    }
    ensure(seen_zero, "应出现回绕块号 0")?;
    Ok(())
}

/// 大于 32 MiB 的文件在默认块大小下回绕块号并完整传完。
///
/// # Errors
///
/// 回绕行为或字节数不符时返回错误。
#[test]
fn large_file_wraps() -> TestResult {
    let len = 33 * 1024 * 1024 + 17;
    let data: Arc<[u8]> = Arc::from(vec![7_u8; len]);
    let options = opts(&[("windowsize", "16")]);
    let mut session = Session::new(data, &options, Limits::default());
    let _oack = session.start();
    let mut action = session.on_packet(&Packet::Ack { block: 0 });
    let mut wraps = 0_u32;
    loop {
        let sent = blocks(&action)?;
        wraps = wraps.saturating_add(u32::try_from(
            sent.iter().filter(|block| **block == 0).count(),
        )?);
        let Some(last) = sent.last().copied() else {
            return Err(Box::new(TestFailure::new("空窗口")));
        };
        action = session.on_packet(&Packet::Ack { block: last });
        if action == Action::Done {
            break;
        }
    }
    ensure_eq(&wraps, &1, "33 MiB / 512 B 回绕一次")?;
    ensure_eq(
        &session.acked_bytes(),
        &u64::try_from(len)?,
        "确认字节数等于文件长度",
    )?;
    ensure_eq(&Limits::for_mtu(1500).largest_block, &1468, "MTU 1500")?;
    ensure_eq(
        &Limits::for_mtu(9000).largest_block,
        &1468,
        "巨帧仍按 1468 封顶",
    )?;
    ensure_eq(&Limits::for_mtu(576).largest_block, &544, "MTU 576")?;
    Ok(())
}

/// 对端错误或非法报文终止传输。
///
/// # Errors
///
/// 行为不符时返回错误。
#[test]
fn errors_abort() -> TestResult {
    let data: Arc<[u8]> = Arc::from(vec![0_u8; 10]);
    let mut session = Session::new(Arc::clone(&data), &[], Limits::default());
    let _first = session.start();
    ensure_eq(
        &session.on_packet(&Packet::Error {
            code: crate::packet::ErrorCode::Undefined,
            message: String::new(),
        }),
        &Action::Abort(None),
        "对端错误",
    )?;
    let mut other = Session::new(data, &[], Limits::default());
    let _again = other.start();
    ensure(
        matches!(other.on_packet(&Packet::Write), Action::Abort(Some(_))),
        "非法报文",
    )?;
    Ok(())
}
