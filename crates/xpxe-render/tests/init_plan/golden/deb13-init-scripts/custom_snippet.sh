#!/bin/bash
# x-pxe 初始化模块 custom_snippet：自定义片段
set -euo pipefail

dir="$(mktemp -d)"
trap 'rm -rf "${dir}"' EXIT
