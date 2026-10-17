<#
.SYNOPSIS
    构建 DotWallpaper 微软商店 MSIX 安装包。
.DESCRIPTION
    1) 商店模式构建：VITE_STORE_BUILD=1，剥离应用内自更新入口，且不生成更新产物；
    2) 组装 MSIX 包布局：DotWallpaper.exe + Package.appxmanifest + Assets；
    3) 调用 winapp 打包为 .msix；-Sign 时附带本地自签证书用于安装验证（提交商店时商店会自行重签）。
.EXAMPLE
    pwsh -File scripts/build-msix.ps1
    pwsh -File scripts/build-msix.ps1 -Sign
#>
[CmdletBinding()]
param(
    [switch]$Sign,
    [string]$CertPath = "src-tauri/devcert.pfx",
    [string]$CertPassword = "password"
)

$ErrorActionPreference = "Stop"

# npx / winapp 会向 stderr 输出进度信息，避免 PowerShell 将其误判为终止错误
if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

if (-not $PSScriptRoot) { throw "请以脚本方式运行本文件" }
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# 解析 winapp CLI：优先 PATH（npm run 会注入 node_modules/.bin），
# 其次回退到根工程 devDependency（@microsoft/winappcli）自带的本地 shim，
# 这样直接 pwsh -File scripts/build-msix.ps1 也能工作。
$winapp = (Get-Command winapp -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $winapp) {
    $localShim = Join-Path $root "node_modules/.bin/winapp.cmd"
    if (Test-Path $localShim) { $winapp = $localShim }
}
if (-not $winapp) {
    throw "未找到 winapp CLI。请在仓库根目录执行 npm install（安装 @microsoft/winappcli 开发依赖）后重试"
}

$manifest   = "src-tauri/Package.appxmanifest"
$packageDir = "src-tauri/target/msix"
$layout     = "$packageDir/layout"

if (-not (Test-Path $manifest)) {
    throw "未找到 $manifest；请先执行：winapp manifest generate src-tauri --package-name <IdentityName> --publisher-name <Publisher DN> --version 0.1.4.0 --entrypoint src-tauri/target/release/dotwallpaper.exe --logo-path src-tauri/icons/icon.png"
}

$version     = (Get-Content "package.json" -Raw | ConvertFrom-Json).version
$msixVersion = "$version.0"

Write-Host "[1/3] 商店模式构建（剥离自更新入口，不产更新产物）" -ForegroundColor Cyan
$env:VITE_STORE_BUILD = 1
# 用 npx.cmd 绕过 PowerShell 脚本包装，避免原生命令 stderr 触发终止
npx.cmd tauri build --config src-tauri/tauri.microsoftstore.conf.json --no-bundle
if ($LASTEXITCODE -ne 0) { throw "tauri build 失败，退出码 $LASTEXITCODE" }

$exe = "src-tauri/target/release/dotwallpaper.exe"
if (-not (Test-Path $exe)) { throw "未找到构建产物：$exe" }

Write-Host "[2/3] 组装包布局 -> $layout" -ForegroundColor Cyan
if (Test-Path $layout) { Remove-Item $layout -Recurse -Force }
New-Item -ItemType Directory -Path $layout -Force | Out-Null

Copy-Item $exe -Destination $layout
Copy-Item "src-tauri/Assets" -Destination $layout -Recurse -Force
Copy-Item $manifest -Destination $layout

Write-Host "[3/3] 打包 MSIX" -ForegroundColor Cyan
$outFile = "$packageDir/xiaohai.DotWallpaper_${msixVersion}_x64.msix"
$pkgArgs = @("package", $layout, "--manifest", $manifest, "--output", $outFile)

if ($Sign) {
    # 证书 Subject 必须与清单 Identity/Publisher 完全一致，否则包无法安装
    $needGen = -not (Test-Path $CertPath)
    if (-not $needGen) {
        $expectedPublisher = ([xml](Get-Content $manifest -Raw)).Package.Identity.Publisher
        try {
            $actualPublisher = (Get-PfxCertificate -FilePath $CertPath -Password (ConvertTo-SecureString $CertPassword -AsPlainText -Force)).Subject
            if ($actualPublisher -ne $expectedPublisher) {
                Write-Host "证书 Publisher（$actualPublisher）与清单（$expectedPublisher）不一致，重新签发" -ForegroundColor Yellow
                $needGen = $true
            }
        } catch {
            Write-Host "既有证书读取失败，重新签发：$($_.Exception.Message)" -ForegroundColor Yellow
            $needGen = $true
        }
    }
    if ($needGen) {
        Write-Host "签发自签开发证书：$CertPath" -ForegroundColor Yellow
        & $winapp cert generate --manifest $manifest --output $CertPath --password $CertPassword --if-exists Overwrite
    }
    $pkgArgs += @("--cert", $CertPath, "--cert-password", $CertPassword)
}

& $winapp @pkgArgs
if ($LASTEXITCODE -ne 0) { throw "MSIX 打包失败，退出码 $LASTEXITCODE" }

Write-Host "构建完成：$outFile" -ForegroundColor Green
if ($Sign) {
    Write-Host "首次安装前需信任证书（管理员执行一次）：npx winapp cert install $CertPath" -ForegroundColor Yellow
}
