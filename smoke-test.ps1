[CmdletBinding()]
param(
    [string]$Exe = "$PSScriptRoot\target\release\fileglyph.exe"
)

$ErrorActionPreference = 'Stop'
$Extension = '.fglyphdemo'
$SecondExtension = '.fglyphdemo2'
$ProgId = 'FileGlyph.DemoFile'
$Classes = 'Registry::HKEY_CURRENT_USER\Software\Classes'
$ExtensionKey = Join-Path $Classes $Extension
$SecondExtensionKey = Join-Path $Classes $SecondExtension
$ProgIdKey = Join-Path $Classes $ProgId

if (-not (Test-Path $Exe)) {
    throw "FileGlyph executable not found at $Exe. Run .\build-windows.ps1 first."
}
if (Test-Path $ExtensionKey) {
    throw "$ExtensionKey already exists. Refusing to overwrite a pre-existing association."
}
if (Test-Path $SecondExtensionKey) {
    throw "$SecondExtensionKey already exists. Refusing to overwrite a pre-existing association."
}
if (Test-Path $ProgIdKey) {
    throw "$ProgIdKey already exists. Refusing to overwrite a pre-existing ProgID."
}

try {
    New-Item -Force -Path $ExtensionKey | Out-Null
    Set-Item -Path $ExtensionKey -Value $ProgId
    New-Item -Force -Path $SecondExtensionKey | Out-Null
    Set-Item -Path $SecondExtensionKey -Value $ProgId

    New-Item -Force -Path $ProgIdKey | Out-Null
    New-Item -Force -Path "$ProgIdKey\shell" | Out-Null
    New-Item -Force -Path "$ProgIdKey\shell\open" | Out-Null
    New-Item -Force -Path "$ProgIdKey\shell\open\command" | Out-Null
    Set-Item -Path "$ProgIdKey\shell\open\command" -Value ('"{0}\System32\notepad.exe" "%1"' -f $env:SystemRoot)

    New-Item -Force -Path "$ProgIdKey\DefaultIcon" | Out-Null
    Set-Item -Path "$ProgIdKey\DefaultIcon" -Value ('"{0}\System32\notepad.exe",0' -f $env:SystemRoot)

    & $Exe refresh

    Write-Host '--- Scan ---'
    $Scan = & $Exe scan --all --extensions fglyphdemo --format json | ConvertFrom-Json
    if (-not $Scan) {
        throw 'The temporary extension was not returned by scan.'
    }
    $Record = @($Scan) | Where-Object extension -eq $Extension | Select-Object -First 1
    if (-not $Record) {
        throw "The scan did not contain $Extension."
    }
    if ($Record.assessment -ne 'inherited_executable') {
        throw "Expected inherited_executable, got $($Record.assessment)."
    }

    Write-Host '--- Dry run ---'
    & $Exe apply --extensions fglyphdemo --dry-run

    $DefaultIconKey = "$ExtensionKey\DefaultIcon"
    if (Test-Path $DefaultIconKey) {
        throw 'Dry run unexpectedly created the extension DefaultIcon key.'
    }

    Write-Host '--- Apply ---'
    & $Exe apply --extensions fglyphdemo,fglyphdemo2 --yes
    if (-not (Test-Path $DefaultIconKey)) {
        throw 'Apply did not create the extension DefaultIcon key.'
    }
    $AppliedValue = (Get-Item $DefaultIconKey).GetValue('')
    if ($AppliedValue -notmatch 'FileGlyph.*icons.*fglyphdemo\.ico') {
        throw "Unexpected applied value: $AppliedValue"
    }
    $IconPath = [regex]::Match($AppliedValue, '^"?(.*?)"?,0$').Groups[1].Value
    if (-not (Test-Path -LiteralPath $IconPath)) {
        throw "Apply did not create the generated ICO: $IconPath"
    }
    [byte[]]$IconBytes = Get-Content -LiteralPath $IconPath -AsByteStream
    if ($IconBytes.Length -lt 6 -or [BitConverter]::ToUInt16($IconBytes, 0) -ne 0 -or [BitConverter]::ToUInt16($IconBytes, 2) -ne 1) {
        throw 'The generated icon does not have a valid ICO header.'
    }
    if ([BitConverter]::ToUInt16($IconBytes, 4) -ne 10) {
        throw 'The generated icon did not contain the configured ten image entries.'
    }
    $ExpectedSizes = @(16, 20, 24, 32, 40, 48, 64, 96, 128, 256)
    $ActualSizes = for ($Index = 0; $Index -lt 10; $Index++) {
        $Width = $IconBytes[6 + ($Index * 16)]
        if ($Width -eq 0) { 256 } else { [int]$Width }
    }
    if ((Compare-Object $ExpectedSizes $ActualSizes -SyncWindow 0)) {
        throw "Unexpected generated ICO sizes: $($ActualSizes -join ', ')"
    }
    $SecondDefaultIconKey = "$SecondExtensionKey\DefaultIcon"
    if (-not (Test-Path $SecondDefaultIconKey)) {
        throw 'Apply did not create the second extension DefaultIcon key.'
    }

    Write-Host '--- Status ---'
    $Status = & $Exe status --format json | ConvertFrom-Json
    $Managed = @($Status) | Where-Object extension -eq $Extension | Select-Object -First 1
    if (-not $Managed -or -not $Managed.confirmed) {
        throw 'Status did not contain a confirmed recovery record.'
    }
    $SecondManaged = @($Status) | Where-Object extension -eq $SecondExtension | Select-Object -First 1
    if (-not $SecondManaged -or $SecondManaged.mechanism -ne 'icon_handler') {
        throw 'The shared ProgID did not use the dynamic icon handler for both extensions.'
    }

    Write-Host '--- Restore ---'
    & $Exe restore --extensions fglyphdemo,fglyphdemo2 --yes
    if (Test-Path $DefaultIconKey) {
        $Restored = (Get-Item $DefaultIconKey).GetValue('')
        if ($null -ne $Restored -and $Restored -ne '') {
            throw "Restore left an unexpected value: $Restored"
        }
    }
    if (Test-Path $SecondDefaultIconKey) {
        $SecondRestored = (Get-Item $SecondDefaultIconKey).GetValue('')
        if ($null -ne $SecondRestored -and $SecondRestored -ne '') {
            throw "Restore left an unexpected second value: $SecondRestored"
        }
    }

    Write-Host 'Smoke test passed.'
}
finally {
    Remove-Item -Recurse -Force $ExtensionKey -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $SecondExtensionKey -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $ProgIdKey -ErrorAction SilentlyContinue
    if (Test-Path $Exe) {
        & $Exe refresh | Out-Null
    }
}
