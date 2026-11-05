#!/usr/bin/env bash
# 发布用（release.yml 调用）：在安装包目录生成 SHA256SUMS.txt 与发布说明 notes.md。
# 用法：bash scripts/release-notes.sh <安装包目录>
set -euo pipefail
cd "$1"

# Windows 上的 sha256sum 按二进制模式读，输出“哈希 *文件名”；统一成常见的两个空格
sha256sum -- *-setup.exe | sed 's/ \*/  /' > SHA256SUMS.txt

{
  echo "Windows 安装包（当前用户安装，不需要管理员权限）。"
  echo
  echo "### SHA256"
  echo
  echo '```'
  cat SHA256SUMS.txt
  echo '```'
  echo
  echo "下载后在安装包所在目录打开 PowerShell 核对，输出 True 即文件完整："
  echo
  echo '```powershell'
  while read -r hash name; do
    echo "(Get-FileHash .\\$name).Hash -eq '$hash'"
  done < SHA256SUMS.txt
  echo '```'
} > notes.md
