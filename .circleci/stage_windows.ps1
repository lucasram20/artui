# Stage a Windows release archive after `cargo build --release`.
#
# Usage: stage_windows.ps1 <rust-target-triple> <asset-label>
#   rust-target-triple - e.g. x86_64-pc-windows-msvc
#   asset-label        - e.g. windows-x86_64
#
# Reads the version from /tmp/release-meta/version (populated by the
# resolve_tag command) and stages a .zip under dist/.
#
# NOTE: this file is intentionally pure ASCII. CircleCI's Windows runner
# defaults to PowerShell 5.1, which reads non-BOM UTF-8 as ANSI/CP1252
# and mangles multi-byte chars (em-dashes, smart quotes) - the result
# is a "string is missing the terminator" parser error two lines down
# from the offending byte. Keep this file ASCII or save with UTF-8 BOM.

param(
    [Parameter(Mandatory = $true)][string]$TargetTriple,
    [Parameter(Mandatory = $true)][string]$AssetLabel
)

$ErrorActionPreference = 'Stop'

$versionPath = '/tmp/release-meta/version'
if (-not (Test-Path $versionPath)) {
    # On Windows the BSD-style /tmp/ path works via Git Bash but
    # PowerShell may need the C: equivalent. Try alternates before
    # bailing.
    $candidates = @(
        "$env:TEMP\release-meta\version",
        "C:\tmp\release-meta\version"
    )
    $found = $false
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            $versionPath = $candidate
            $found = $true
            break
        }
    }
    if (-not $found) {
        Write-Error "ERROR: release-meta/version missing - resolve_tag step did not run"
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

$ZipPath = "dist/$Name.zip"
Compress-Archive -Path $DistDir -DestinationPath $ZipPath -Force
Remove-Item $DistDir -Recurse -Force
Get-ChildItem dist
