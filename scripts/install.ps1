# artui install script — Windows PowerShell
#
# Usage:
#   irm https://artui.dev/install.ps1 | iex
#   irm https://artui.dev/install.ps1 | iex -ArgumentList -Version v0.0.1

[CmdletBinding()]
param(
    [string]$Version = $env:ARTUI_VERSION ?? 'latest',
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'artui\bin'),
    [string]$Repo = $env:ARTUI_REPO ?? 'lucasram20/artui'
)

$ErrorActionPreference = 'Stop'

$arch = if ([Environment]::Is64BitOperatingSystem) { 'x86_64' } else { 'i686' }
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { $arch = 'aarch64' }

$target = "$arch-pc-windows-msvc"

if ($Version -eq 'latest') {
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $release.tag_name
}

$asset = "artui-$($Version.TrimStart('v'))-$target.zip"
$url = "https://github.com/$Repo/releases/download/$Version/$asset"

Write-Host "Downloading $url"
$tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP "artui-$([guid]::NewGuid())") -Force
$zip = Join-Path $tmp 'artui.zip'
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
Expand-Archive -Path $zip -DestinationPath $tmp -Force

$exe = Get-ChildItem -Path $tmp -Filter 'artui.exe' -Recurse | Select-Object -First 1
if (-not $exe) {
    throw "Could not locate artui.exe inside the downloaded archive."
}

New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
$dest = Join-Path $InstallDir 'artui.exe'
Copy-Item $exe.FullName $dest -Force

Remove-Item -Recurse -Force $tmp

# Add to user PATH if not already present.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not ($userPath -split ';' | Where-Object { $_ -ieq $InstallDir })) {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$InstallDir", 'User')
    Write-Host "Added $InstallDir to your User PATH (open a new terminal to pick it up)."
}

Write-Host "Installed artui $Version to $dest"
