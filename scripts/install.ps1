# Install OxiDNS release archives on Windows.
#
# Common overrides:
#   $env:OXIDNS_VERSION = "v1.0.1"
#   $env:OXIDNS_INSTALL_DIR = "C:\OxiDNS"
#   $env:OXIDNS_TARGET = "x86_64-pc-windows-msvc"
#   $env:OXIDNS_BUNDLE = "full"
#   $env:OXIDNS_INSTALL_SERVICE = "0"
#   $env:OXIDNS_START_SERVICE = "0"

param(
    [string]$Version = $env:OXIDNS_VERSION,
    [string]$Repository = $env:OXIDNS_REPO,
    [string]$Target = $env:OXIDNS_TARGET,
    [string]$Bundle = $env:OXIDNS_BUNDLE,
    [string]$InstallDir = $env:OXIDNS_INSTALL_DIR,
    [string]$NoPath = $env:OXIDNS_NO_PATH,
    [string]$InstallService = $env:OXIDNS_INSTALL_SERVICE,
    [string]$StartService = $env:OXIDNS_START_SERVICE
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Test-Truthy {
    param([string]$Value)
    return $Value -match "^(1|true|yes|on)$"
}

function Write-Info {
    param([string]$Message)
    Write-Host $Message
}

function Get-OxiDnsTarget {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch ($arch) {
        "AMD64" { return "x86_64-pc-windows-msvc" }
        "ARM64" { return "aarch64-pc-windows-msvc" }
        "x86"   { return "i686-pc-windows-msvc" }
        default {
            throw "unsupported Windows architecture: $arch. Set OXIDNS_TARGET to override."
        }
    }
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Add-PathEntry {
    param(
        [string]$PathToAdd,
        [string]$Scope
    )

    $current = [Environment]::GetEnvironmentVariable("Path", $Scope)
    if ([string]::IsNullOrWhiteSpace($current)) {
        [Environment]::SetEnvironmentVariable("Path", $PathToAdd, $Scope)
        $env:Path = "$PathToAdd;$env:Path"
        return
    }

    $trimChars = [char[]]@("\", "/")
    $entries = $current -split ";" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    foreach ($entry in $entries) {
        if ([string]::Equals($entry.TrimEnd($trimChars), $PathToAdd.TrimEnd($trimChars), [StringComparison]::OrdinalIgnoreCase)) {
            return
        }
    }

    [Environment]::SetEnvironmentVariable("Path", "$current;$PathToAdd", $Scope)
    $env:Path = "$PathToAdd;$env:Path"
}

if (-not [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
    throw "scripts/install.ps1 is for Windows. Use scripts/install.sh on Linux or macOS."
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = "latest"
}
if ([string]::IsNullOrWhiteSpace($Repository)) {
    $Repository = "svenshi/oxidns"
}
if ([string]::IsNullOrWhiteSpace($InstallService)) {
    $InstallService = "1"
}
if ([string]::IsNullOrWhiteSpace($StartService)) {
    $StartService = "1"
}
if ([string]::IsNullOrWhiteSpace($Bundle)) {
    $Bundle = "full"
}
$Bundle = $Bundle.ToLowerInvariant()
if ($Bundle -ne "full") {
    throw "Windows release archives are only published for OXIDNS_BUNDLE=full"
}
$serviceInstall = Test-Truthy $InstallService
$serviceStart = Test-Truthy $StartService
if ($serviceInstall -and -not (Test-Administrator)) {
    if (-not [string]::IsNullOrWhiteSpace($env:OXIDNS_INSTALL_SERVICE)) {
        throw "service installation requires an elevated PowerShell session; rerun as Administrator or set OXIDNS_INSTALL_SERVICE=0 for a user install"
    }
    Write-Info "Note: not running as Administrator; falling back to user install (no Windows service)."
    Write-Info "To install as a service, rerun from an elevated PowerShell session."
    $serviceInstall = $false
    $serviceStart = $false
}
if ([string]::IsNullOrWhiteSpace($Target)) {
    $Target = Get-OxiDnsTarget
}
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    if ($serviceInstall) {
        $base = $env:ProgramFiles
        if ([string]::IsNullOrWhiteSpace($base)) {
            $base = "C:\Program Files"
        }
    } else {
        $base = $env:LOCALAPPDATA
        if ([string]::IsNullOrWhiteSpace($base)) {
            $base = Join-Path $HOME "AppData\Local"
        }
    }
    $InstallDir = Join-Path $base "OxiDNS"
}

if ($Target -notlike "*windows*" -and $Target -notlike "*msvc*") {
    throw "non-Windows targets are installed with scripts/install.sh"
}

$asset = "oxidns-$Target.zip"
if ($Version -eq "latest") {
    $url = "https://github.com/$Repository/releases/latest/download/$asset"
} else {
    $url = "https://github.com/$Repository/releases/download/$Version/$asset"
}

if ([enum]::GetNames([Net.SecurityProtocolType]) -contains "Tls12") {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("oxidns-install-" + [Guid]::NewGuid().ToString("N"))
$archivePath = Join-Path $tempRoot $asset
$unpackDir = Join-Path $tempRoot "unpack"

New-Item -ItemType Directory -Path $tempRoot, $unpackDir -Force | Out-Null

try {
    Write-Info "Downloading $asset from $Repository ($Version)..."
    $downloadArgs = @{
        Uri = $url
        OutFile = $archivePath
    }
    if ((Get-Command Invoke-WebRequest).Parameters.ContainsKey("UseBasicParsing")) {
        $downloadArgs.UseBasicParsing = $true
    }
    Invoke-WebRequest @downloadArgs

    Expand-Archive -Path $archivePath -DestinationPath $unpackDir -Force

    $exeSource = Join-Path $unpackDir "oxidns.exe"
    $configSource = Join-Path $unpackDir "config.yaml"
    if (-not (Test-Path -LiteralPath $exeSource)) {
        throw "archive does not contain oxidns.exe"
    }
    if (-not (Test-Path -LiteralPath $configSource)) {
        throw "archive does not contain config.yaml"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    $exePath = Join-Path $InstallDir "oxidns.exe"
    $configPath = Join-Path $InstallDir "config.yaml"
    $tempExePath = Join-Path $InstallDir "oxidns.exe.tmp"
    Copy-Item -LiteralPath $exeSource -Destination $tempExePath -Force
    Move-Item -LiteralPath $tempExePath -Destination $exePath -Force

    if (Test-Path -LiteralPath $configPath) {
        $examplePath = Join-Path $InstallDir "config.yaml.example"
        Copy-Item -LiteralPath $configSource -Destination $examplePath -Force
        Write-Info "Keeping existing config: $configPath"
        Write-Info "Wrote release example config: $examplePath"
    } else {
        Copy-Item -LiteralPath $configSource -Destination $configPath -Force
    }

    $licenseSource = Join-Path $unpackDir "LICENSE"
    if (Test-Path -LiteralPath $licenseSource) {
        Copy-Item -LiteralPath $licenseSource -Destination (Join-Path $InstallDir "LICENSE") -Force
    }

    $webuiSource = Join-Path $unpackDir "webui"
    if (Test-Path -LiteralPath $webuiSource) {
        $webuiDest = Join-Path $InstallDir "webui"
        if (Test-Path -LiteralPath $webuiDest) {
            Remove-Item -LiteralPath $webuiDest -Recurse -Force
        }
        Copy-Item -LiteralPath $webuiSource -Destination $webuiDest -Recurse -Force
    }

    $pathScope = if ($serviceInstall) { "Machine" } else { "User" }
    if (-not (Test-Truthy $NoPath)) {
        Add-PathEntry -PathToAdd $InstallDir -Scope $pathScope
    }

    & $exePath check -c $configPath -d $InstallDir *> $null
    if ($LASTEXITCODE -eq 0) {
        Write-Info "Config check passed: $configPath"
    } else {
        Write-Warning "installed binary is ready, but config check failed: $configPath"
    }

    if ($serviceInstall) {
        & $exePath service install -d $InstallDir -c $configPath
        if ($LASTEXITCODE -ne 0) {
            throw "service installation failed"
        }
        if ($serviceStart) {
            & $exePath service start
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "Service was installed but failed to start automatically."
                Write-Warning "To start it manually, run from an elevated PowerShell:"
                Write-Warning "  oxidns.exe service start"
                Write-Warning "Or check the Windows Event Log for details."
            }
        }
    }

    Write-Info "OxiDNS installed to $InstallDir"
    if (-not (Test-Truthy $NoPath)) {
        Write-Info "Added install directory to $pathScope PATH. Open a new PowerShell window if oxidns is not found."
    }
    Write-Info "Try: oxidns.exe start -c `"$configPath`" -d `"$InstallDir`""
} finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
