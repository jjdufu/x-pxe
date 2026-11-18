//! “复用现有 DHCP”模式下给现网 DHCP 服务器的配置片段。
//!
//! 现网 DHCP 负责分配地址并下发引导参数，bootnet 只提供 TFTP：
//! - BIOS PXE（Opt93 = 0）：`next-server = 本机`，`filename = undionly.kpxe`；
//! - UEFI PXE（Opt93 = 7/9）：`next-server = 本机`，`filename = ipxe.efi`；
//! - UEFI HTTP Boot（Opt60 以 `HTTPClient` 开头）：`filename = <基址>/fw/x64/ipxe.efi`，Opt60 回 `HTTPClient`；
//! - iPXE（Opt77 = `iPXE` 或携带 Opt175）：`filename = <基址>/boot.ipxe`，该脚本链式加载
//!   `/boot/mac/${net0/mac:hexhyp}`，非 `Armed` 节点得到 `exit`。
//!
//! iPXE 判断必须排在最前，否则 iPXE 会再次拿到固件文件名而循环加载。

use std::net::Ipv4Addr;

use xpxe_domain::HttpUrl;

/// 现网 DHCP 服务器种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerKind {
    /// ISC Kea（JSON 配置的 `client-classes`）。
    Kea,
    /// ISC dhcpd（`dhcpd.conf`）。
    IscDhcpd,
    /// Windows Server DHCP（PowerShell 策略）。
    WindowsDhcp,
    /// `MikroTik` `RouterOS` 7（`matcher` 与 `option-set`）。
    RouterOs,
}

impl ServerKind {
    /// 全部种类，按 Web 显示顺序。
    pub const ALL: [Self; 4] = [Self::Kea, Self::IscDhcpd, Self::WindowsDhcp, Self::RouterOs];

    /// 稳定的蛇形名。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Kea => "kea",
            Self::IscDhcpd => "isc_dhcpd",
            Self::WindowsDhcp => "windows_dhcp",
            Self::RouterOs => "routeros",
        }
    }
}

/// 一段配置片段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    /// 服务器种类。
    pub kind: ServerKind,
    /// 片段文本。
    pub text: String,
}

/// 片段所需的地址。
struct Targets {
    /// TFTP 服务器（本机在引导网卡上的地址）。
    server: Ipv4Addr,
    /// iPXE 脚本 URL。
    script: String,
    /// HTTP Boot 固件 URL。
    http_fw: String,
}

impl Targets {
    /// 由基址拼出 URL。
    fn new(server: Ipv4Addr, boot_base: &HttpUrl) -> Self {
        let base = boot_base.as_str().trim_end_matches('/');
        Self {
            server,
            script: format!("{base}/boot.ipxe"),
            http_fw: format!("{base}/fw/x64/ipxe.efi"),
        }
    }
}

/// 渲染四种服务器的片段。
#[must_use]
pub fn render_snippets(server: Ipv4Addr, boot_base: &HttpUrl) -> Vec<Snippet> {
    let targets = Targets::new(server, boot_base);
    ServerKind::ALL
        .iter()
        .map(|kind| Snippet {
            kind: *kind,
            text: match kind {
                ServerKind::Kea => kea(&targets),
                ServerKind::IscDhcpd => isc_dhcpd(&targets),
                ServerKind::WindowsDhcp => windows(&targets),
                ServerKind::RouterOs => routeros(&targets),
            },
        })
        .collect()
}

/// Kea `client-classes`（与实验室 X1/X2 验证过的配置同构）。
fn kea(targets: &Targets) -> String {
    let Targets {
        server,
        script,
        http_fw,
    } = targets;
    format!(
        r#""client-classes": [
  {{
    "name": "xpxe-ipxe",
    "test": "option[77].text == 'iPXE' or option[175].exists",
    "boot-file-name": "{script}"
  }},
  {{
    "name": "xpxe-uefi-http",
    "test": "not member('xpxe-ipxe') and substring(option[60].hex,0,10) == 'HTTPClient'",
    "boot-file-name": "{http_fw}",
    "option-data": [ {{ "name": "vendor-class-identifier", "data": "HTTPClient" }} ]
  }},
  {{
    "name": "xpxe-uefi-pxe",
    "test": "not member('xpxe-ipxe') and not member('xpxe-uefi-http') and (option[93].hex == 0x0007 or option[93].hex == 0x0009)",
    "next-server": "{server}",
    "boot-file-name": "ipxe.efi"
  }},
  {{
    "name": "xpxe-bios-pxe",
    "test": "not member('xpxe-ipxe') and option[93].hex == 0x0000",
    "next-server": "{server}",
    "boot-file-name": "undionly.kpxe"
  }}
]
"#
    )
}

/// ISC dhcpd `dhcpd.conf` 片段。
fn isc_dhcpd(targets: &Targets) -> String {
    let Targets {
        server,
        script,
        http_fw,
    } = targets;
    format!(
        r#"option arch code 93 = unsigned integer 16;
option ipxe-encap code 175 = encapsulate ipxe;

if exists user-class and option user-class = "iPXE" {{
  filename "{script}";
}} elsif exists ipxe-encap {{
  filename "{script}";
}} elsif substring(option vendor-class-identifier, 0, 10) = "HTTPClient" {{
  option vendor-class-identifier "HTTPClient";
  filename "{http_fw}";
}} elsif option arch = 00:07 or option arch = 00:09 {{
  next-server {server};
  filename "ipxe.efi";
}} else {{
  next-server {server};
  filename "undionly.kpxe";
}}
"#
    )
}

/// Windows Server DHCP（PowerShell，按处理顺序建立策略）。
fn windows(targets: &Targets) -> String {
    let Targets {
        server,
        script,
        http_fw,
    } = targets;
    format!(
        r#"# 在 DHCP 服务器上以管理员身份运行；-ScopeId 换成目标作用域
$Scope = "192.0.2.0"
Add-DhcpServerv4Class -Name "xpxe-iPXE" -Type User -Data "iPXE"
Add-DhcpServerv4Class -Name "xpxe-HTTPClient" -Type Vendor -Data "HTTPClient"
Add-DhcpServerv4Class -Name "xpxe-UEFI-x64" -Type Vendor -Data "PXEClient:Arch:00007"
Add-DhcpServerv4Class -Name "xpxe-BIOS" -Type Vendor -Data "PXEClient:Arch:00000"

Add-DhcpServerv4Policy -ScopeId $Scope -Name "xpxe-ipxe" -ProcessingOrder 1 -Condition OR -UserClass EQ,"xpxe-iPXE"
Set-DhcpServerv4OptionValue -ScopeId $Scope -PolicyName "xpxe-ipxe" -OptionId 67 -Value "{script}"

Add-DhcpServerv4Policy -ScopeId $Scope -Name "xpxe-uefi-http" -ProcessingOrder 2 -Condition OR -VendorClass EQ,"xpxe-HTTPClient*"
Set-DhcpServerv4OptionValue -ScopeId $Scope -PolicyName "xpxe-uefi-http" -OptionId 60 -Value "HTTPClient"
Set-DhcpServerv4OptionValue -ScopeId $Scope -PolicyName "xpxe-uefi-http" -OptionId 67 -Value "{http_fw}"

Add-DhcpServerv4Policy -ScopeId $Scope -Name "xpxe-uefi-pxe" -ProcessingOrder 3 -Condition OR -VendorClass EQ,"xpxe-UEFI-x64*"
Set-DhcpServerv4OptionValue -ScopeId $Scope -PolicyName "xpxe-uefi-pxe" -OptionId 66 -Value "{server}"
Set-DhcpServerv4OptionValue -ScopeId $Scope -PolicyName "xpxe-uefi-pxe" -OptionId 67 -Value "ipxe.efi"

Add-DhcpServerv4Policy -ScopeId $Scope -Name "xpxe-bios-pxe" -ProcessingOrder 4 -Condition OR -VendorClass EQ,"xpxe-BIOS*"
Set-DhcpServerv4OptionValue -ScopeId $Scope -PolicyName "xpxe-bios-pxe" -OptionId 66 -Value "{server}"
Set-DhcpServerv4OptionValue -ScopeId $Scope -PolicyName "xpxe-bios-pxe" -OptionId 67 -Value "undionly.kpxe"
"#
    )
}

/// `MikroTik` `RouterOS` 7：`matcher` 按 Opt77/Opt60 选 `option-set`，默认网段下发 BIOS 固件。
fn routeros(targets: &Targets) -> String {
    let Targets {
        server,
        script,
        http_fw,
    } = targets;
    format!(
        r#"# RouterOS 7；server= 换成现网 DHCP 服务名，address= 换成引导网段
/ip dhcp-server option
add name=xpxe-ipxe-file code=67 value="'{script}'"
add name=xpxe-http-class code=60 value="'HTTPClient'"
add name=xpxe-http-file code=67 value="'{http_fw}'"
add name=xpxe-tftp-server code=66 value="'{server}'"
add name=xpxe-efi-file code=67 value="'ipxe.efi'"
/ip dhcp-server option sets
add name=xpxe-ipxe options=xpxe-ipxe-file
add name=xpxe-uefi-http options=xpxe-http-class,xpxe-http-file
add name=xpxe-uefi-pxe options=xpxe-tftp-server,xpxe-efi-file
/ip dhcp-server matcher
add name=xpxe-ipxe server=defconf code=77 value="iPXE" option-set=xpxe-ipxe
add name=xpxe-uefi-http server=defconf code=60 value="HTTPClient" matching-type=substring option-set=xpxe-uefi-http
add name=xpxe-uefi-pxe server=defconf code=60 value="PXEClient:Arch:00007" matching-type=substring option-set=xpxe-uefi-pxe
/ip dhcp-server network
set [find address="192.0.2.0/24"] next-server={server} boot-file-name=undionly.kpxe
"#
    )
}

#[cfg(test)]
mod tests;
