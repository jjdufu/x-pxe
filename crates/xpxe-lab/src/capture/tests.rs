//! pcap 解析测试。

use std::net::Ipv4Addr;

use xpxe_test_support::{TestResult, ensure, ensure_eq};

use super::parse_pcap;

/// 构造一个只含一帧 UDP 的 pcap。
fn pcap_with(sport: u16, dport: u16, payload: &[u8]) -> Vec<u8> {
    let mut udp = Vec::new();
    udp.extend_from_slice(&sport.to_be_bytes());
    udp.extend_from_slice(&dport.to_be_bytes());
    let udp_len = u16::try_from(payload.len().saturating_add(8)).unwrap_or(u16::MAX);
    udp.extend_from_slice(&udp_len.to_be_bytes());
    udp.extend_from_slice(&[0, 0]);
    udp.extend_from_slice(payload);
    let total = u16::try_from(udp.len().saturating_add(20)).unwrap_or(u16::MAX);
    let mut ip = vec![0x45, 0];
    ip.extend_from_slice(&total.to_be_bytes());
    ip.extend_from_slice(&[0, 0, 0, 0, 64, 17, 0, 0]);
    ip.extend_from_slice(&[0, 0, 0, 0]);
    ip.extend_from_slice(&[255, 255, 255, 255]);
    ip.extend_from_slice(&udp);
    let mut frame = vec![0xFF; 6];
    frame.extend_from_slice(&[0x52, 0x54, 0, 0x77, 1, 1]);
    frame.extend_from_slice(&[0x08, 0x00]);
    frame.extend_from_slice(&ip);
    let mut file = Vec::new();
    file.extend_from_slice(&0xA1B2_C3D4_u32.to_le_bytes());
    file.extend_from_slice(&[2, 0, 4, 0]);
    file.extend_from_slice(&[0; 8]);
    file.extend_from_slice(&65535_u32.to_le_bytes());
    file.extend_from_slice(&1_u32.to_le_bytes());
    let len = u32::try_from(frame.len()).unwrap_or(0);
    file.extend_from_slice(&7_u32.to_le_bytes());
    file.extend_from_slice(&0_u32.to_le_bytes());
    file.extend_from_slice(&len.to_le_bytes());
    file.extend_from_slice(&len.to_le_bytes());
    file.extend_from_slice(&frame);
    file
}

/// DHCP 端口的报文被提取，其他端口被忽略，坏文件头被拒绝。
///
/// # Errors
///
/// 结果不符时返回错误。
#[test]
fn extracts_dhcp_udp() -> TestResult {
    let packets = parse_pcap(&pcap_with(68, 67, b"hello"))?;
    ensure_eq(&packets.len(), &1, "数量")?;
    let first = packets.first().ok_or("缺少报文")?;
    ensure_eq(&first.payload, &b"hello".to_vec(), "负载")?;
    ensure_eq(&first.dst, &Ipv4Addr::BROADCAST, "目的")?;
    ensure_eq(&first.seconds, &7, "时间")?;
    ensure(
        parse_pcap(&pcap_with(5353, 5353, b"x"))?.is_empty(),
        "非 DHCP 端口",
    )?;
    ensure(
        parse_pcap(b"not a pcap at all, definitely").is_err(),
        "坏文件头",
    )?;
    Ok(())
}
