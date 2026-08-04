[CmdletBinding()]
param(
    [switch]$SkipTests,
    [switch]$SkipFormat
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Push-Location $Root
try {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'cargo was not found. Install the stable Rust MSVC toolchain from rustup.rs.'
    }

    cargo --version
    if ($LASTEXITCODE -ne 0) { throw "cargo --version failed with exit code $LASTEXITCODE" }
    rustc --version
    if ($LASTEXITCODE -ne 0) { throw "rustc --version failed with exit code $LASTEXITCODE" }

    if (-not $SkipFormat) {
        cargo fmt --all
        if ($LASTEXITCODE -ne 0) { throw "cargo fmt failed with exit code $LASTEXITCODE" }
    }
    if (-not $SkipTests) {
        cargo test --workspace --all-targets
        if ($LASTEXITCODE -ne 0) { throw "cargo test failed with exit code $LASTEXITCODE" }
    }

    cargo build --workspace --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }

    $Dist = Join-Path $Root 'dist'
    New-Item -ItemType Directory -Force -Path $Dist | Out-Null
    Copy-Item (Join-Path $Root 'target\release\fileglyph.exe') $Dist -Force
    Copy-Item (Join-Path $Root 'target\release\fileglyph-gui.exe') $Dist -Force
    Copy-Item (Join-Path $Root 'target\release\fileglyph_icon_handler.dll') $Dist -Force
    Copy-Item (Join-Path $Root 'README.md') $Dist -Force
    Copy-Item (Join-Path $Root 'LICENSE') $Dist -Force
    Copy-Item (Join-Path $Root 'VALIDATION.md') $Dist -Force
    Copy-Item (Join-Path $Root 'config.example.json') $Dist -Force

    Write-Host "Built: $Dist\fileglyph.exe"
    Write-Host "Built: $Dist\fileglyph-gui.exe"
    Write-Host "Built: $Dist\fileglyph_icon_handler.dll"
}
finally {
    Pop-Location
}
