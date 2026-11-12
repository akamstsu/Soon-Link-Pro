[CmdletBinding()]
param([string]$OutputDirectory = (Join-Path $PSScriptRoot '..\build\tools'))
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$downloads = @(
    @{
        Name = 'usbip-win2-0.9.8.1-x64.exe'
        Url = 'https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe'
        Hash = '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a'
    },
    @{
        Name = 'innosetup-6.4.3.exe'
        Url = 'https://github.com/jrsoftware/issrc/releases/download/is-6_4_3/innosetup-6.4.3.exe'
        Hash = 'f3c42116542c4cc57263c5ba6c4feabfc49fe771f2f98a79d2f7628b8762723b'
    }
)
foreach ($download in $downloads) {
    $path = Join-Path $OutputDirectory $download.Name
    if (-not (Test-Path -LiteralPath $path) -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $download.Hash) {
        Invoke-WebRequest -Uri $download.Url -OutFile $path -MaximumRetryCount 3 -RetryIntervalSec 3 -TimeoutSec 180
    }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $download.Hash) {
        throw "SHA256 mismatch: $($download.Name)"
    }
}
# This downloads build inputs only; it never runs a driver or modifies the system.
