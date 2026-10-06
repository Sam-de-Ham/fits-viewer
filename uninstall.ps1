<#
.SYNOPSIS
    Removes FITS Viewer: unregisters it and deletes "C:\Program Files\FITS Viewer".
    Re-launches itself as administrator.
#>

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`""
    return
}

$dir = "$env:ProgramFiles\FITS Viewer"
if (Test-Path "$dir\fits_viewer.dll") {
    Start-Process regsvr32 -ArgumentList "/s /u `"$dir\fits_viewer.dll`"" -Wait
}
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue

if (Test-Path $dir) {
    Write-Host "Unregistered. Windows still has the DLL loaded; delete $dir after a restart." -ForegroundColor Yellow
} else {
    Write-Host 'FITS Viewer removed.' -ForegroundColor Green
}
Read-Host 'Press Enter to close'
