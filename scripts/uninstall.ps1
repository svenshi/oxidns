# Uninstall OxiDNS files installed by scripts/install.ps1 on Windows.
#
# Common overrides:
#   $env:OXIDNS_INSTALL_DIR = "C:\OxiDNS"
#   $env:OXIDNS_UNINSTALL_SERVICE = "1"
#   $env:OXIDNS_PURGE = "1"

param(
    [string]$InstallDir = $env:OXIDNS_INSTALL_DIR,
    [string]$NoPath = $env:OXIDNS_NO_PATH,
    [string]$UninstallService = $env:OXIDNS_UNINSTALL_SERVICE,
    [string]$Purge = $env:OXIDNS_PURGE
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Test-Truthy {
    param([string]$Value)
    return $Value -match "^(1|true|yes|on)$"
}

function Write-Info {
    param([string]$Message)
    Write-Host $Message
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Test-ShouldUninstallService {
    param([string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -eq "auto") {
        return (Test-Administrator)
    }
    return (Test-Truthy $Value)
}

function Remove-PathEntry {
    param(
        [string]$PathToRemove,
        [string]$Scope
    )

    $current = [Environment]::GetEnvironmentVariable("Path", $Scope)
    if ([string]::IsNullOrWhiteSpace($current)) {
        return
    }

    $trimChars = [char[]]@("\", "/")
    $normalizedRemove = $PathToRemove.TrimEnd($trimChars)
    $entries = $current -split ";" | Where-Object {
        -not [string]::IsNullOrWhiteSpace($_) -and
        -not [string]::Equals($_.TrimEnd($trimChars), $normalizedRemove, [StringComparison]::OrdinalIgnoreCase)
    }

    $newPath = [string]::Join(";", $entries)
    if ($newPath -ne $current) {
        [Environment]::SetEnvironmentVariable("Path", $newPath, $Scope)
        $env:Path = [string]::Join(";", ($env:Path -split ";" | Where-Object {
            -not [string]::Equals($_.TrimEnd($trimChars), $normalizedRemove, [StringComparison]::OrdinalIgnoreCase)
        }))
        Write-Info "Removed install directory from $Scope PATH"
    }
}

function Assert-SafePurgePath {
    param([string]$PathToPurge)

    $trimChars = [char[]]@("\", "/")
    $fullPath = [System.IO.Path]::GetFullPath($PathToPurge).TrimEnd($trimChars)
    $homePath = [System.IO.Path]::GetFullPath($HOME).TrimEnd($trimChars)
    $localAppData = $env:LOCALAPPDATA
    if (-not [string]::IsNullOrWhiteSpace($localAppData)) {
        $localAppData = [System.IO.Path]::GetFullPath($localAppData).TrimEnd($trimChars)
    }

    $unsafe = @(
        [System.IO.Path]::GetPathRoot($fullPath).TrimEnd($trimChars),
        $homePath,
        $localAppData,
        [Environment]::GetFolderPath("ProgramFiles").TrimEnd($trimChars),
        [Environment]::GetFolderPath("ProgramFilesX86").TrimEnd($trimChars),
        [Environment]::GetFolderPath("Windows").TrimEnd($trimChars)
    ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }

    foreach ($entry in $unsafe) {
        if ([string]::Equals($fullPath, $entry, [StringComparison]::OrdinalIgnoreCase)) {
            throw "refusing to purge unsafe install directory: $PathToPurge"
        }
    }
}

if (-not [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
    throw "scripts/uninstall.ps1 is for Windows. Use scripts/uninstall.sh on Linux or macOS."
}

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    if (Test-ShouldUninstallService $UninstallService) {
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

$exePath = Join-Path $InstallDir "oxidns.exe"

if (Test-ShouldUninstallService $UninstallService) {
    if (Test-Path -LiteralPath $exePath) {
        # Service may already be stopped (exit code 1062); ignore stop failures.
        try { & $exePath service stop *> $null } catch { }
        & $exePath service uninstall *> $null
        if ($LASTEXITCODE -eq 0) {
            Write-Info "Removed OxiDNS service"
        } else {
            Write-Warning "service uninstall failed or service was not installed"
        }
    } else {
        Write-Warning "cannot uninstall service because $exePath was not found"
    }
}

if (-not (Test-Truthy $NoPath)) {
    $pathScope = if (Test-ShouldUninstallService $UninstallService) { "Machine" } else { "User" }
    Remove-PathEntry -PathToRemove $InstallDir -Scope $pathScope
}

if (Test-Truthy $Purge) {
    Assert-SafePurgePath -PathToPurge $InstallDir
    if (Test-Path -LiteralPath $InstallDir) {
        Remove-Item -LiteralPath $InstallDir -Recurse -Force
        Write-Info "Purged OxiDNS install directory: $InstallDir"
    }
} else {
    $paths = @(
        (Join-Path $InstallDir "oxidns.exe"),
        (Join-Path $InstallDir "oxidns.exe.tmp"),
        (Join-Path $InstallDir "LICENSE")
    )
    foreach ($path in $paths) {
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force
        }
    }

    $webuiPath = Join-Path $InstallDir "webui"
    if (Test-Path -LiteralPath $webuiPath) {
        Remove-Item -LiteralPath $webuiPath -Recurse -Force
    }

    Write-Info "Removed OxiDNS binary and WebUI from $InstallDir"
    $configPath = Join-Path $InstallDir "config.yaml"
    if (Test-Path -LiteralPath $configPath) {
        Write-Info "Kept config: $configPath"
        Write-Info "Use OXIDNS_PURGE=1 to remove the install directory and config."
    }
}

Write-Info "OxiDNS uninstall complete"
