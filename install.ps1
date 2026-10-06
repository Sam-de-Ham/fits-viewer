<#
.SYNOPSIS
    Installs or updates FITS Viewer for all users.
.DESCRIPTION
    Copies fits_viewer.dll to "C:\Program Files\FITS Viewer" and registers it. The DLL is taken
    from next to this script, or from target\release after `cargo build --release`.
    Re-launches itself as administrator.
#>

$ErrorActionPreference = 'Stop'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`""
    return
}

$source = "$PSScriptRoot\fits_viewer.dll", "$PSScriptRoot\target\release\fits_viewer.dll" |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $source) { throw 'fits_viewer.dll not found. Build it first with: cargo build --release' }

$dir = "$env:ProgramFiles\FITS Viewer"
$target = "$dir\fits_viewer.dll"
New-Item -ItemType Directory -Force $dir | Out-Null
Remove-Item "$dir\*.old.dll" -ErrorAction SilentlyContinue

if (Test-Path $target) {
    # A DLL that Windows still has loaded can't be overwritten, but it can be renamed.
    Rename-Item $target "fits_viewer.$([DateTime]::Now.Ticks).old.dll"
}
Copy-Item $source $target

$regsvr = Start-Process regsvr32 -ArgumentList "/s `"$target`"" -Wait -PassThru
if ($regsvr.ExitCode -ne 0) { throw "Registration failed (regsvr32 exit code $($regsvr.ExitCode))" }

Write-Host "FITS Viewer installed to $dir" -ForegroundColor Green
Write-Host 'Thumbnails: View > Large icons. Preview pane: Alt+P.'
Write-Host 'Full size: right-click > Open with > Windows Photo Viewer or Photos (FITS).'
Read-Host 'Press Enter to close'
