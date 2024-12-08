Write-Host "Installing GTK4 and dependencies for $env:VCPKG_TRIPLET..."

# Install pkg-config first
vcpkg install pkgconf:$env:VCPKG_TRIPLET
$pkgConfigPath = "$env:VCPKG_ROOT\installed\$env:VCPKG_TRIPLET\tools\pkgconf\pkgconf.exe"

# Verify pkg-config installation
if (Test-Path $pkgConfigPath) {
    Write-Host "pkg-config installed successfully at: $pkgConfigPath"
} else {
    Write-Host "Error: pkg-config installation failed"
    exit 1
}

# Install other dependencies
vcpkg install gobject-2.0:$env:VCPKG_TRIPLET
vcpkg install glib:$env:VCPKG_TRIPLET
vcpkg install gtk4:$env:VCPKG_TRIPLET
vcpkg install libadwaita:$env:VCPKG_TRIPLET

# Create pkg-config wrapper script to handle Windows paths
$wrapperContent = @"
#!/usr/bin/env pwsh
`$env:PKG_CONFIG_PATH = `$env:PKG_CONFIG_PATH -replace ';',':'
`$env:PKG_CONFIG_LIBDIR = `$env:PKG_CONFIG_LIBDIR -replace ';',':'
& '$pkgConfigPath' @args
"@

$batchContent = @"
@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ""%~dp0pkg-config.ps1"" %*
"@

$wrapperPath = "$env:VCPKG_ROOT\installed\$env:VCPKG_TRIPLET\tools\pkgconf\pkg-config.ps1"
$batchPath = "$env:VCPKG_ROOT\installed\$env:VCPKG_TRIPLET\tools\pkgconf\pkg-config.bat"

$wrapperContent | Out-File -FilePath $wrapperPath -Encoding UTF8
$batchContent | Out-File -FilePath $batchPath -Encoding ASCII

Write-Host "Created pkg-config wrapper at: $wrapperPath"
Write-Host "Created pkg-config batch wrapper at: $batchPath"