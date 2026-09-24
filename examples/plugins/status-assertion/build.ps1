# 构建 status-assertion 插件为 WASM 组件并打包为 orbit zip 插件（Windows）。
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$target = "wasm32-wasip2"
$pluginDir = "com.example.status-assertion"

rustup target add $target | Out-Null
cargo build --target $target --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

if (Test-Path $pluginDir) { Remove-Item -Recurse -Force $pluginDir }
New-Item -ItemType Directory -Path $pluginDir | Out-Null
Copy-Item manifest.json "$pluginDir/manifest.json"
Copy-Item "target/$target/release/status_assertion.wasm" "$pluginDir/status_assertion.wasm"

if (Test-Path "com.example.status-assertion.zip") { Remove-Item "com.example.status-assertion.zip" }
Compress-Archive -Path $pluginDir -DestinationPath "com.example.status-assertion.zip" -CompressionLevel Optimal

Write-Host "OK -> com.example.status-assertion.zip (可在插件管理页安装)"
