[CmdletBinding()]
param(
    [switch]$MakeInstaller,
    [string]$InnoCompiler,
    [string]$DriverInstaller
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$driverHash = '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a'
if ($MakeInstaller) {
    if (-not $InnoCompiler -or -not (Test-Path -LiteralPath $InnoCompiler -PathType Leaf)) {
        throw 'Pass -InnoCompiler with the path to Inno Setup 6 ISCC.exe.'
    }
    if (-not $DriverInstaller -or -not (Test-Path -LiteralPath $DriverInstaller -PathType Leaf)) {
        throw 'Pass -DriverInstaller with the usbip-win2 0.9.8.1 x64 installer.'
    }
    $InnoCompiler = (Resolve-Path -LiteralPath $InnoCompiler).Path
    $DriverInstaller = (Resolve-Path -LiteralPath $DriverInstaller).Path
    if ((Get-FileHash -LiteralPath $DriverInstaller -Algorithm SHA256).Hash.ToLowerInvariant() -ne $driverHash) {
        throw 'The driver installer SHA256 does not match the pinned release.'
    }
}
Push-Location -LiteralPath $repoRoot
try {
    $metadata = & cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
    $version = ($metadata.packages | Where-Object name -eq 'soon-link-pro').version
    if ($version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') { throw 'Invalid application version.' }
    $numericVersion = $version.Split('-')[0]
    & cargo build --locked --release --workspace --target-dir (Join-Path $repoRoot 'target')
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed: $LASTEXITCODE" }
    if (-not $MakeInstaller) { return }
    $payload = Join-Path $repoRoot 'build\payload'
    $output = Join-Path $repoRoot 'dist'
    foreach ($name in @('', 'drivers', 'licenses', 'assets')) {
        New-Item -ItemType Directory -Path (Join-Path $payload $name) -Force | Out-Null
    }
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    foreach ($name in @('SoonLinkPro.exe', 'SoonLinkProMcp.exe', 'patchbay_asio.dll')) {
        Copy-Item -LiteralPath (Join-Path $repoRoot "target\release\$name") -Destination $payload -Force
    }
    Copy-Item -LiteralPath $DriverInstaller -Destination (Join-Path $payload 'drivers\usbip-win2-0.9.8.1-x64.exe') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'assets\soon-link-pro-icon-v2.ico') -Destination (Join-Path $payload 'assets') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'assets\THIRD_PARTY_NOTICES.md') -Destination (Join-Path $payload 'licenses') -Force
    foreach ($name in @('LICENSE', 'NOTICE')) {
        Copy-Item -LiteralPath (Join-Path $repoRoot $name) -Destination (Join-Path $payload 'licenses') -Force
    }
    # Inno Setup uses the BOM to display the UTF-8 copyright line correctly.
    [IO.File]::WriteAllText((Join-Path $payload 'licenses\LICENSE'), [IO.File]::ReadAllText((Join-Path $repoRoot 'LICENSE')), [Text.UTF8Encoding]::new($true))
    Copy-Item -LiteralPath (Join-Path $repoRoot 'assets\usbip-win2-LICENSE.txt') -Destination (Join-Path $payload 'licenses') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'vendor\usbip-audio\LICENSE') -Destination (Join-Path $payload 'licenses\usbip-audio-LICENSE.txt') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'assets\upstream-licenses') -Destination (Join-Path $payload 'licenses') -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'WINDOWS_AUDIO.md') -Destination (Join-Path $payload '使用说明.md') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\MCP.md') -Destination (Join-Path $payload 'MCP使用说明.md') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\RELEASE_NOTES.md') -Destination (Join-Path $payload '本次更新.md') -Force
    & $InnoCompiler "/DPayloadDir=$payload" "/DInstallerOutput=$output" "/DAppVersion=$numericVersion" "/DAppDisplayVersion=$version" (Join-Path $repoRoot 'installer\soon-link-pro.iss')
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed: $LASTEXITCODE" }
}
finally {
    Pop-Location
}
