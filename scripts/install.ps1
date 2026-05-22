# artui install script — Windows PowerShell
#
# Usage:
#   irm https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.ps1 | iex
#   irm https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.ps1 | iex -ArgumentList -Version v0.3.5

[CmdletBinding()]
param(
    [string]$Version = $env:ARTUI_VERSION ?? 'latest',
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'artui\bin'),
    [string]$Repo = $env:ARTUI_REPO ?? 'lucasram20/artui',
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'
$IsInteractive = $Host.UI.RawUI -and -not $env:CI -and -not $env:NO_COLOR
$AssumeYes = $Yes -or ($env:ARTUI_INSTALL_YES -eq '1')

# Public Cloudflare R2 mirror — primary download source. Lets users
# without GitHub access install zero-auth.
$R2Base = if ($env:ARTUI_MIRROR_BASE) { $env:ARTUI_MIRROR_BASE } else { 'https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev' }

# Optional GitHub token for private-repo access.
$Token = if ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } elseif ($env:GH_TOKEN) { $env:GH_TOKEN } else { $null }
$AuthHeaders = @{}
if ($Token) {
    $AuthHeaders['Authorization'] = "Bearer $Token"
    $AuthHeaders['Accept'] = 'application/vnd.github+json'
}

function Write-Logo {
    if (-not $IsInteractive) { Write-Host 'artui installer'; return }
    $logo = @(
        '  █████╗ ██████╗ ████████╗██╗   ██╗██╗',
        ' ██╔══██╗██╔══██╗╚══██╔══╝██║   ██║██║',
        ' ███████║██████╔╝   ██║   ██║   ██║██║',
        ' ██╔══██║██╔══██╗   ██║   ██║   ██║██║',
        ' ██║  ██║██║  ██║   ██║   ╚██████╔╝██║',
        ' ╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝'
    )
    foreach ($line in $logo) { Write-Host $line -ForegroundColor Cyan }
    Write-Host '  interactive coding-agent CLI' -ForegroundColor DarkGray
    Write-Host ''
}

function Step($msg)    { Write-Host "› $msg" -ForegroundColor DarkGray }
function Success($msg) { Write-Host "✔ $msg" -ForegroundColor Green }
function Warn($msg)    { Write-Host "! $msg" -ForegroundColor Yellow }
function Fail($msg)    { Write-Host "✖ $msg" -ForegroundColor Red }

# Run a scriptblock while a PowerShell native progress bar ticks. Native
# Write-Progress already animates and respects the host capabilities; we
# just wrap it to keep the visual style consistent.
function Invoke-WithProgress {
    param([string]$Activity, [scriptblock]$Body)
    if (-not $IsInteractive) { Step $Activity; & $Body; return }
    Write-Progress -Activity $Activity -Status 'Working…' -PercentComplete -1
    try { & $Body } finally { Write-Progress -Activity $Activity -Completed }
    Success $Activity
}

Write-Logo

# Confirmation — skip when -Yes, ARTUI_INSTALL_YES=1, CI, or non-interactive.
function Confirm-Install {
    if ($AssumeYes -or $env:CI) { return $true }
    if (-not $IsInteractive) {
        Warn 'No interactive terminal detected; pass -Yes to bypass this prompt.'
        return $false
    }
    Write-Host ''
    $reply = Read-Host "Install artui to $InstallDir? [Y/n]"
    return ($reply -eq '' -or $reply -match '^(y|yes)$')
}

if (-not (Confirm-Install)) {
    Warn 'Install aborted.'
    exit 0
}

$arch = if ([Environment]::Is64BitOperatingSystem) { 'x86_64' } else { 'i686' }
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { $arch = 'aarch64' }

$target = "$arch-pc-windows-msvc"
Step "Target $target"

if ($Version -eq 'latest') {
    Step 'Resolving latest release'
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers $AuthHeaders
    } catch {
        Fail "Could not resolve latest release for $Repo."
        if (-not $Token) { Warn 'Repo may be private. Set $env:GITHUB_TOKEN to a fine-grained PAT (Contents:read, Metadata:read).' }
        exit 1
    }
    $Version = $release.tag_name
}
Step "Version $Version"

$asset = "artui-$($Version.TrimStart('v'))-$target.zip"
$publicUrl = "https://github.com/$Repo/releases/download/$Version/$asset"

$tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP "artui-$([guid]::NewGuid())") -Force
$zip = Join-Path $tmp 'artui.zip'

Invoke-WithProgress -Activity "Downloading $asset" -Body {
    $r2Url = "$R2Base/$Version/$asset"
    # Try the public R2 mirror first.
    try {
        Invoke-WebRequest -Uri $r2Url -OutFile $zip -UseBasicParsing -ErrorAction Stop
        return
    } catch {
        Step "R2 mirror miss; falling back to GitHub"
    }
    if ($Token) {
        $tagRelease = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$Version" -Headers $AuthHeaders
        $assetMeta = $tagRelease.assets | Where-Object { $_.name -eq $asset } | Select-Object -First 1
        if (-not $assetMeta) { throw "Asset $asset not found on release $Version." }
        $assetUrl = "https://api.github.com/repos/$Repo/releases/assets/$($assetMeta.id)"
        $headers = $AuthHeaders.Clone()
        $headers['Accept'] = 'application/octet-stream'
        Invoke-WebRequest -Uri $assetUrl -OutFile $zip -Headers $headers -UseBasicParsing
    } else {
        Invoke-WebRequest -Uri $publicUrl -OutFile $zip -UseBasicParsing
    }
}

Invoke-WithProgress -Activity 'Extracting archive' -Body {
    Expand-Archive -Path $zip -DestinationPath $tmp -Force
}

$exe = Get-ChildItem -Path $tmp -Filter 'artui.exe' -Recurse | Select-Object -First 1
if (-not $exe) {
    Fail 'Could not locate artui.exe inside the downloaded archive.'
    exit 1
}

New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
$dest = Join-Path $InstallDir 'artui.exe'
Copy-Item $exe.FullName $dest -Force
Remove-Item -Recurse -Force $tmp

# Add to user PATH if not already present.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not ($userPath -split ';' | Where-Object { $_ -ieq $InstallDir })) {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$InstallDir", 'User')
    Warn "Added $InstallDir to your User PATH (open a new terminal to pick it up)."
}

Success "Installed artui $Version to $dest"
