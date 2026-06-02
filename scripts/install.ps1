# artui install script — Windows PowerShell
#
# Usage:
#   irm https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.ps1 | iex
#   irm https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.ps1 | iex -ArgumentList -Version v0.7.0
#
# Resolves the latest version from the public Cloudflare R2 mirror first
# (zero-auth, works for everyone). Falls back to the GitHub API when R2 is
# unreachable; the GitHub path needs $env:GITHUB_TOKEN if the source repo
# is private.

[CmdletBinding()]
param(
    [string]$Version = $(if ($env:ARTUI_VERSION) { $env:ARTUI_VERSION } else { 'latest' }),
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'artui\bin'),
    [string]$Repo = $(if ($env:ARTUI_REPO) { $env:ARTUI_REPO } else { 'lucasram20/artui' }),
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
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    # Windows-on-ARM (Snapdragon X, etc.) runs x86_64 binaries
    # transparently via Microsoft's emulator since 2020. Serving the
    # x86_64 archive there is the correct default — native ARM64 binaries
    # aren't published because the user base is small and CircleCI
    # macOS concurrency makes a full 6-target matrix budget-prohibitive.
    Warn 'Windows ARM64 detected; using the x86_64 binary via the built-in emulator.'
    $arch = 'x86_64'
}

$target = "windows-$arch"
Step "Target $target"

if ($Version -eq 'latest') {
    Step 'Resolving latest release'
    $ghTag = $null
    $r2Tag = $null

    # GitHub is authoritative for the release tag (R2 `latest/` can lag if mirror upload failed).
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers $AuthHeaders -UseBasicParsing -ErrorAction Stop
        $ghTag = $release.tag_name
    } catch {
        # Non-fatal; try R2 below.
    }

    try {
        $checksums = Invoke-RestMethod -Uri "$R2Base/latest/checksums.sha256" -UseBasicParsing -ErrorAction Stop
        $match = [regex]::Match($checksums, 'artui-([0-9]+\.[0-9]+\.[0-9]+)-(?:linux|macos|windows)')
        if ($match.Success) {
            $r2Tag = "v$($match.Groups[1].Value)"
        }
    } catch {
        # R2 miss is non-fatal when GitHub succeeded.
    }

    if ($ghTag) {
        $resolved = $ghTag
        if ($r2Tag -and $r2Tag -ne $ghTag) {
            Warn "R2 mirror latest ($r2Tag) differs from GitHub ($ghTag); using GitHub."
        }
    } elseif ($r2Tag) {
        $resolved = $r2Tag
        Warn "GitHub latest unavailable; using R2 mirror tag $r2Tag."
    } else {
        Fail "Could not resolve latest release for $Repo from GitHub or R2 mirror."
        if (-not $Token) {
            Warn 'Repo may be private. Set $env:GITHUB_TOKEN to a fine-grained PAT (Contents:read, Metadata:read).'
        }
        Warn "Or pin a specific version: irm $R2Base/install.ps1 | iex -ArgumentList -Version v0.7.0"
        exit 1
    }

    $Version = $resolved
}
Step "Version $Version"

$asset = "artui-$($Version.TrimStart('v'))-$target.zip"
$publicUrl = "https://github.com/$Repo/releases/download/$Version/$asset"

$tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP "artui-$([guid]::NewGuid())") -Force
$zip = Join-Path $tmp 'artui.zip'

Invoke-WithProgress -Activity "Downloading $asset" -Body {
    $r2Url = "$R2Base/$Version/$asset"
    # Try the public R2 mirror first — works for everyone, no auth.
    try {
        Invoke-WebRequest -Uri $r2Url -OutFile $zip -UseBasicParsing -ErrorAction Stop
        return
    } catch {
        Step "R2 mirror miss; falling back to GitHub"
    }
    if ($Token) {
        # Authenticated path: resolve the asset id, then hit the API
        # asset endpoint with `Accept: application/octet-stream` so
        # GitHub returns a signed redirect we can follow.
        $tagRelease = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$Version" -Headers $AuthHeaders -UseBasicParsing
        $assetMeta = $tagRelease.assets | Where-Object { $_.name -eq $asset } | Select-Object -First 1
        if (-not $assetMeta) { throw "Asset $asset not found on release $Version." }
        $assetUrl = "https://api.github.com/repos/$Repo/releases/assets/$($assetMeta.id)"
        $headers = $AuthHeaders.Clone()
        $headers['Accept'] = 'application/octet-stream'
        Invoke-WebRequest -Uri $assetUrl -OutFile $zip -Headers $headers -UseBasicParsing
    } else {
        # Last resort: unauthenticated public release URL. Fails on
        # private repos — the caller should set GITHUB_TOKEN if they
        # need the GitHub fallback while the repo is private.
        try {
            Invoke-WebRequest -Uri $publicUrl -OutFile $zip -UseBasicParsing -ErrorAction Stop
        } catch {
            throw "Both R2 ($r2Url) and public GitHub ($publicUrl) failed. Set `$env:GITHUB_TOKEN to a fine-grained PAT (Contents:read, Metadata:read), or check that the version exists on the R2 mirror."
        }
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
