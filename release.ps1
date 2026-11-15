# Builds the plugin + installer and publishes a GitHub release (the plugin's updater picks it up).
# Usage: bump `version` in Cargo.toml, commit, then run:  powershell -File release.ps1
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$ver = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (git tag --list "v$ver") { throw "Tag v$ver already exists - bump the version in Cargo.toml first." }
if (git status --porcelain) { throw 'Uncommitted changes - commit them first.' }

cargo xtask bundle clean_voice --release
if ($LASTEXITCODE) { throw 'Plugin build failed' }

$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 not found (winget install JRSoftware.InnoSetup)' }
& $iscc /Q "/DAppVersion=$ver" installer\clean_voice.iss
if ($LASTEXITCODE) { throw 'Installer build failed' }

git tag "v$ver"
git push origin HEAD "v$ver"
# Copy with a fixed name: the Yaytsa Cloud site links to .../releases/latest/download/CleanVoice-Setup.exe
Copy-Item "dist\CleanVoice-$ver-setup.exe" "dist\CleanVoice-Setup.exe" -Force
gh release create "v$ver" "dist\CleanVoice-$ver-setup.exe" "dist\CleanVoice-Setup.exe" --title "Clean Voice $ver" --generate-notes
Write-Host "Released v$ver"
