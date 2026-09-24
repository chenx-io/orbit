# 构建 echo-protocol 插件为 WASM 组件并打包为 orbit zip 插件（Windows）。
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$target = "wasm32-wasip2"
$pluginDir = "com.example.echo"

rustup target add $target | Out-Null
cargo build --target $target --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

# 组装插件目录
if (Test-Path $pluginDir) { Remove-Item -Recurse -Force $pluginDir }
New-Item -ItemType Directory -Path $pluginDir | Out-Null
Copy-Item manifest.json "$pluginDir/manifest.json"
Copy-Item "target/$target/release/echo_protocol.wasm" "$pluginDir/echo.wasm"

# 打包 zip（顶层即插件目录）
if (Test-Path "com.example.echo.zip") { Remove-Item "com.example.echo.zip" }
Compress-Archive -Path $pluginDir -DestinationPath "com.example.echo.zip" -CompressionLevel Optimal

Write-Host "OK -> com.example.echo.zip (可在插件管理页安装)"
