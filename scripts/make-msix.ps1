# Builds the Microsoft Store package (.msix) from the release build, the way
# Watt's Left ships to the Store: the package goes up unsigned and the Store
# signs it, so there is no certificate to buy.
#
#   npx tauri build --bundles nsis          (or just cargo build --release)
#   powershell -ExecutionPolicy Bypass -File scripts\make-msix.ps1
#
# Upload the .msix it prints in Partner Center > the app > Packages.
# Before the first upload, copy Package/Identity/Name from Partner Center >
# the app > Product identity into -IdentityName (or the default below).
# Test locally (Developer Mode on): -Install registers the layout folder as
# an installed app, no certificate needed; -Uninstall removes it again.

param(
  [string]$IdentityName = 'AronFrishberg.NeptuneVoiceDictation',
  [string]$Publisher = 'CN=E021B26C-F581-4C65-BDF8-02510788F0A0',
  [string]$PublisherDisplayName = 'Aron Frishberg',
  [string]$DisplayName = 'Neptune: Voice Dictation',
  [switch]$Install,
  [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$out = Join-Path $root 'src-tauri\target\release\bundle\msix'
$layout = Join-Path $out 'layout'

if ($Uninstall) {
  Get-AppxPackage -Name $IdentityName | Remove-AppxPackage
  'removed'
  return
}

# the version: tauri.conf.json's three parts, plus the 0 the Store wants
$version = ((Get-Content (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version) + '.0'
$exe = Join-Path $root 'src-tauri\target\release\neptune.exe'
if (-not (Test-Path $exe)) { throw "no release build at $exe; build first" }

# the newest Windows SDK tools
$bin = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\10.*\x64\makeappx.exe' | Sort-Object FullName | Select-Object -Last 1
if (-not $bin) { throw 'makeappx.exe not found: install the Windows SDK' }
$makeappx = $bin.FullName
$makepri = Join-Path $bin.DirectoryName 'makepri.exe'

# the layout: the exe, the icons, the manifest with its values filled in
if (Test-Path $layout) { Remove-Item $layout -Recurse -Force }
New-Item -ItemType Directory -Force $layout | Out-Null
Copy-Item $exe $layout
Copy-Item (Join-Path $root 'msix\Assets') (Join-Path $layout 'Assets') -Recurse
$m = Get-Content (Join-Path $root 'msix\AppxManifest.xml') -Raw
$m = $m.Replace('{{IDENTITY_NAME}}', $IdentityName).Replace('{{PUBLISHER}}', $Publisher).Replace('{{VERSION}}', $version)
$m = $m.Replace('{{DISPLAY_NAME}}', [Security.SecurityElement]::Escape($DisplayName)).Replace('{{PUBLISHER_DISPLAY_NAME}}', [Security.SecurityElement]::Escape($PublisherDisplayName))
[IO.File]::WriteAllText((Join-Path $layout 'AppxManifest.xml'), $m, (New-Object Text.UTF8Encoding $false))

# resources.pri: how Windows finds the right icon size, including the
# unplated taskbar icons (no coloured square behind the planet)
$cfg = Join-Path $out 'priconfig.xml'
& $makepri createconfig /cf $cfg /dq en-US /pv 10.0.0 /o | Out-Null
& $makepri new /pr $layout /cf $cfg /of (Join-Path $layout 'resources.pri') /o | Out-Null
if ($LASTEXITCODE) { throw 'makepri failed' }

if ($Install) {
  Get-AppxPackage -Name $IdentityName | Remove-AppxPackage
  Add-AppxPackage -Register (Join-Path $layout 'AppxManifest.xml')
  $pkg = Get-AppxPackage -Name $IdentityName
  "installed $($pkg.PackageFullName)"
  "start it: explorer.exe shell:AppsFolder\$($pkg.PackageFamilyName)!App"
  return
}

$msix = Join-Path $out "Neptune_${version}_x64.msix"
& $makeappx pack /d $layout /p $msix /o | Out-Null
if ($LASTEXITCODE) { throw 'makeappx failed' }
"built $msix ($([math]::Round((Get-Item $msix).Length / 1MB, 1)) MB)"
