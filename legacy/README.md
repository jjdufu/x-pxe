# legacy

现网使用过的装机与初始化脚本，只作为 x-pxe 装机参数与初始化模块的需求来源和实验对照，不参与构建。

- `pxe-server.sh`：旧版 dnsmasq + TFTP + preseed 装机脚本
- `init-d13.sh`：Debian 13 装机后的初始化脚本，其全部功能已拆成 x-pxe 的初始化模块

入库时脚本里的明文口令已替换为占位符。iPXE 固件（`ipxe.efi`、`undionly.kpxe`）不入库，运行时在 WebUI 的资产页下载官方版本或离线上传。
