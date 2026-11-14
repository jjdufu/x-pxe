# x-pxe

x-pxe 是一个 Debian 13（trixie）自动装机平台：一套 Rust 程序加一个 Web 控制台，把裸机或虚拟机从“上电”带到“系统装好并完成初始化”，全程不依赖宿主机上的 dnsmasq、xorriso、mtools、grub、curl、openssl 等系统组件。

- 局域网 PXE（BIOS / UEFI x64）、UEFI HTTP Boot、ProxyDHCP、复用现网 DHCP、Redfish Virtual Media（仅 UEFI）
- Rust 实现的 ProxyDHCP 与只读 TFTP，引导应答由控制面状态机实时决定，只有 `Armed` 节点会得到应答
- iPXE 与 Debian netboot 资产不内置，在 Web 上下载或上传、校验、激活、金丝雀、回滚
- `init-d13.sh` 的全部功能拆成有类型的初始化模块，在 Web 上编辑、预览、对比、回滚
- 控制面镜像 `FROM scratch`，只有静态 musl 二进制和 Web 静态文件

## 文档

| 文件 | 说明 |
| :--- | :--- |
| [`docs/plan/README.md`](docs/plan/README.md) | 总体设计与步骤索引 |
| [`docs/rust-lint-gate.md`](docs/rust-lint-gate.md) | Rust 最严 Lint 门禁（约束性） |
| [`docs/webui-style-guide.md`](docs/webui-style-guide.md) | WebUI 风格与交互规范（约束性） |
| [`docs/webui-ref/exhtml/`](docs/webui-ref/exhtml/) | WebUI 参考实现 |
| [`legacy/`](legacy/) | 现网脚本，需求来源 |

## 许可证

MIT OR Apache-2.0
