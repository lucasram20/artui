# Stage a Windows release archive after `cargo build --release`.
#
# Usage: stage_windows.ps1 <rust-target-triple> <asset-label>
#   rust-target-triple — e.g. x86_64-pc-windows-msvc
#   asset-label        — e.g. windows-x86_64
#
# Reads the version from /tmp/release-meta/version (populated by the
# resolve_tag command) and stages a .zip under dist/.

param(
    [Parameter(Mandatory = $true)][string]$TargetTriple,
    [Parameter(Mandatory = $true)][string]$AssetLabel
)

$ErrorActionPreference = 'Stop'

$versionPath = '/tmp/release-meta/version'
if (-not (Test-Path $versionPath)) {
    # On Windows the BSD-style /tmp/ path does work via Git Bash but PowerShell
    # may need the C: equivalent. Try the alternate path before giving up.
    $altPath = "$env:TEMP\release-meta\version"
    if (Test-Path $altPath) {
        $versionPath = $altPath
    }
    else {
        Write-Error "ERROR: $versionPath missing — resolve_tag step did not run"
        exit 1
    }
}

$Version = (Get-Content $versionPath -Raw).Trim()
$Name = "artui-$Version-$AssetLabel"
$DistDir = "dist/$Name"

New-Item -ItemType Directory -Force $DistDir | Out-Null
Copy-Item "target/$TargetTriple/release/artui.exe" "$DistDir/artui.exe"
if (Test-Path "README.md") { Copy-Item README.md "$DistDir/" }
if (Test-Path "LICENSE") { Copy-Item LICENSE "$DistDir/" }
if (Test-Path "LICENSE-MIT") { Copy-Item LICENSE-MIT "$DistDir/" }
if (Test-Path "LICENSE-APACHE") { Copy-Item LICENSE-APACHE "$DistDir/" }

Compress-Archive -Path $DistDir -DestinationPath "dist/$Name.zip" -Force
Remove-Item $DistDir -Recurse -Force
Get-ChildItem dist
