# Installs or updates what the Windows test VM needs to build, test and run AnvilCAD.
# Run through scripts/winvm/winvm provision; safe to run again.
param([string]$Password)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$dl = 'C:\anvil\downloads'
New-Item -ItemType Directory -Force $dl | Out-Null

# Keep a desktop session signed in and unlocked, so the windowed app can be driven and
# screenshotted (scripts/winvm/session.ps1). This is a disposable test VM on a private LAN.
$winlogon = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
if ($Password) {
  Set-ItemProperty $winlogon AutoAdminLogon '1'
  Set-ItemProperty $winlogon DefaultUserName $env:USERNAME
  Set-ItemProperty $winlogon DefaultPassword $Password
  Remove-ItemProperty $winlogon AutoLogonCount -ErrorAction SilentlyContinue
}
New-Item -Force 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization' | Out-Null
Set-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization' NoLockScreen 1 -Type DWord
Set-ItemProperty 'HKCU:\Control Panel\Desktop' ScreenSaveActive '0'
powercfg /change standby-timeout-ac 0
powercfg /change monitor-timeout-ac 0
Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' LongPathsEnabled 1 -Type DWord

# Defender scanning every object file roughly doubles build times.
Add-MpPreference -ExclusionPath 'C:\anvil', "$env:USERPROFILE\.cargo", "$env:USERPROFILE\.rustup"

# Visual Studio Build Tools: the MSVC linker and Windows SDK that the x86_64-pc-windows-msvc
# toolchain (the one CI uses) needs.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$haveVc = (Test-Path $vswhere) -and (& $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if (-not $haveVc) {
  Write-Output 'Installing Visual Studio Build Tools (C++), this takes 10-20 minutes...'
  Invoke-WebRequest 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile "$dl\vs_BuildTools.exe"
  $p = Start-Process -Wait -PassThru "$dl\vs_BuildTools.exe" -ArgumentList '--quiet', '--wait', '--norestart', '--nocache',
    '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended'
  if ($p.ExitCode -notin 0, 3010) { throw "VS Build Tools installer failed with exit code $($p.ExitCode)" }
} else { Write-Output 'Visual Studio Build Tools: already installed' }

# Rust, through rustup, with the components CI uses.
$cargoBin = "$env:USERPROFILE\.cargo\bin"
if (-not (Test-Path "$cargoBin\rustup.exe")) {
  Write-Output 'Installing Rust...'
  Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile "$dl\rustup-init.exe"
  & "$dl\rustup-init.exe" -y --no-modify-path --default-toolchain stable --profile minimal -c rustfmt -c clippy
  if ($LASTEXITCODE) { throw "rustup-init failed with exit code $LASTEXITCODE" }
} else {
  & "$cargoBin\rustup.exe" update stable --no-self-update
}
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$cargoBin*") { [Environment]::SetEnvironmentVariable('Path', "$cargoBin;$userPath", 'User') }

# Git, for working in the VM by hand. The harness itself copies files over SSH.
if (-not (Get-Command git -ErrorAction SilentlyContinue) -and -not (Test-Path "$env:ProgramFiles\Git\cmd\git.exe")) {
  Write-Output 'Installing Git...'
  try {
    winget install --id Git.Git -e --silent --scope machine --accept-package-agreements --accept-source-agreements --disable-interactivity
  } catch { Write-Output "Git install skipped: $_" }
}

& "$cargoBin\rustc.exe" --version
& "$cargoBin\cargo.exe" --version
Write-Output 'provision: done'
