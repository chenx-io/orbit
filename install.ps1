# ─────────────────────────────────────────────────────────────
# Orbit 官方安装脚本 (Windows / PowerShell)
#
# 用法：
#   一行安装（默认装 orbit-cli）:
#     irm https://raw.githubusercontent.com/chenx-io/orbit/main/install.ps1 | iex
#   带参数安装（先下载再执行）:
#     irm https://raw.githubusercontent.com/chenx-io/orbit/main/install.ps1 -OutFile install.ps1
#     ./install.ps1 -AppOnly            # 只装 App
#     ./install.ps1 -All                # CLI + App
#     ./install.ps1 -Version v0.1.0     # 指定版本
#
# 环境变量：ORBIT_REPO / ORBIT_VERSION 可覆盖默认值
# 下载的压缩包会通过 Release 中的 SHA256SUMS 自动校验完整性
# ─────────────────────────────────────────────────────────────
param(
    [switch]$CliOnly,   # 只装 orbit-cli（默认）
    [switch]$AppOnly,   # 只装 App
    [switch]$All,       # CLI + App
    [string]$Version,   # 版本（默认 latest，如 v0.1.0）
    [string]$Dir        # CLI 安装目录（默认 $env:LOCALAPPDATA\Orbit\bin）
)

$ErrorActionPreference = 'Stop'

# 发布仓库（仓库迁移后改这里即可，脚本本身无需其他改动）
$Repo = if ($env:ORBIT_REPO) { $env:ORBIT_REPO } else { 'chenx-io/orbit' }
$Ver  = if ($Version) { $Version } elseif ($env:ORBIT_VERSION) { $env:ORBIT_VERSION } else { 'latest' }

$InstallCli = $true
$InstallApp = $false
if ($AppOnly) { $InstallCli = $false; $InstallApp = $true }
if ($All)     { $InstallCli = $true;  $InstallApp = $true }

function Write-Log($msg)  { Write-Host $msg -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host $msg -ForegroundColor Green }
function Write-Die($msg)  { Write-Host "✗ $msg" -ForegroundColor Red; exit 1 }

function Get-AssetUrl($asset) {
    if ($Ver -eq 'latest') {
        return "https://github.com/$Repo/releases/latest/download/$asset"
    }
    return "https://github.com/$Repo/releases/download/$Ver/$asset"
}

# App 资产名带版本号，需从 Release 元数据按正则匹配
function Get-AssetNameByPattern($pattern) {
    $api = if ($Ver -eq 'latest') {
        "https://api.github.com/repos/$Repo/releases/latest"
    } else {
        "https://api.github.com/repos/$Repo/releases/tags/$Ver"
    }
    $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'orbit-installer' }
    return ($release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1).name
}

# 用 Release 里的 SHA256SUMS 校验下载的压缩包
function Assert-Checksum($file, $asset) {
    $sums = (Invoke-WebRequest -Uri (Get-AssetUrl 'SHA256SUMS') -UseBasicParsing).Content
    $line = ($sums -split "`r?`n" | Where-Object { $_ -match [regex]::Escape($asset) + '$' } | Select-Object -First 1)
    if (-not $line) { Write-Die "未找到 $asset 的 SHA256SUMS 校验值，请确认版本 $Ver 的 Release 已包含校验文件" }

    $expected = ($line -split '\s+')[0]
    $actual = (Get-FileHash -Path $file -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected.ToLowerInvariant()) {
        Write-Die "校验失败: $asset（期望 $expected，实际 $actual；下载可能损坏，请重试）"
    }
    Write-Ok "🔐 校验通过: $($expected.Substring(0, [Math]::Min(12, $expected.Length)))…"
}

function Install-Cli {
    $binDir = if ($Dir) { $Dir } else { Join-Path $env:LOCALAPPDATA 'Orbit\bin' }
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null

    $asset = "orbit-cli-windows-x86_64.zip"
    $tmp = Join-Path $env:TEMP "orbit-cli-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null

    Write-Log "⬇️  下载 $asset"
    Invoke-WebRequest -Uri (Get-AssetUrl $asset) -OutFile "$tmp\pkg.zip" -UseBasicParsing
    Assert-Checksum "$tmp\pkg.zip" $asset

    Write-Log "📦 解压到 $binDir"
    Expand-Archive -Path "$tmp\pkg.zip" -DestinationPath $tmp -Force
    Copy-Item "$tmp\orbit.exe" $binDir -Force
    Remove-Item $tmp -Recurse -Force

    Write-Ok "✅ orbit-cli 已安装: $binDir\orbit.exe"
    & "$binDir\orbit.exe" --version

    # 写入用户级 PATH（幂等）
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath -notlike "*$binDir*") {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$binDir", 'User')
        Write-Ok "🔧 已追加 PATH（新开终端生效）"
    }
}

function Install-App {
    $exe = Get-AssetNameByPattern 'setup\.exe$'
    if (-not $exe) { Write-Die "未找到 Windows 安装包（-setup.exe）" }

    $tmp = Join-Path $env:TEMP "orbit-app-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    Write-Log "⬇️  下载 $exe"
    Invoke-WebRequest -Uri (Get-AssetUrl $exe) -OutFile "$tmp\setup.exe" -UseBasicParsing
    Assert-Checksum "$tmp\setup.exe" $exe

    Write-Log "🚀 启动安装器（静默安装）..."
    Start-Process -FilePath "$tmp\setup.exe" -ArgumentList '/S' -Wait
    Remove-Item $tmp -Recurse -Force
    Write-Ok "✅ Orbit App 安装完成"
}

Write-Log "🪐 Orbit installer | windows | repo=$Repo | version=$Ver"
if ($InstallCli) { Install-Cli }
if ($InstallApp) { Install-App }
Write-Ok "🎉 安装完成！更多信息: https://github.com/$Repo"
