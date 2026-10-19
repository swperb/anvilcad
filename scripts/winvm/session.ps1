# Acts on the Windows test VM's interactive desktop for scripts/winvm/winvm.
#
# SSH sessions cannot see or drive the desktop, so when this runs over SSH it re-runs itself
# as a scheduled task in the auto-logon user's console session, waits for it and prints what
# it wrote. If someone left that session through Remote Desktop, it is first moved back to
# the console so that it has a screen to draw on.
param(
  [Parameter(Mandatory)][ValidateSet('launch', 'shot', 'click', 'type', 'key', 'windows', 'close', 'resolution')]
  [string]$Action,
  [string]$Exe, [string]$WorkDir, [string[]]$ArgList = @(),
  [string]$Path, [int]$X, [int]$Y, [ValidateSet('left', 'right')][string]$Button = 'left',
  [string]$Text, [string]$Name, [int]$Width, [int]$Height,
  [switch]$InSession, [string]$OutFile
)
$ErrorActionPreference = 'Stop'

function Invoke-AsTask([string]$TaskName, [string]$Command, $Principal) {
  $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -Command $Command"
  $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 5)
  Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $Principal -Settings $settings -Force | Out-Null
  try {
    Start-ScheduledTask -TaskName $TaskName
    $deadline = (Get-Date).AddMinutes(5)
    do { Start-Sleep -Milliseconds 300 } while ((Get-ScheduledTask -TaskName $TaskName).State -eq 'Running' -and (Get-Date) -lt $deadline)
    (Get-ScheduledTaskInfo -TaskName $TaskName).LastTaskResult
  } finally {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
  }
}

if (-not $InSession) {
  $user = $env:USERNAME
  # Find the user's session; reattach it to the console if it is disconnected.
  $line = (quser.exe 2>$null | Select-Object -Skip 1) | Where-Object { $_ -match "^\s*>?$user\s" } | Select-Object -First 1
  if (-not $line) { throw "$user is not logged on to the desktop. Restart the VM (auto-logon signs in) or sign in once over Remote Desktop." }
  $fields = ($line.Trim() -replace '^>', '') -split '\s+'
  $id = ($fields | Where-Object { $_ -match '^\d+$' } | Select-Object -First 1)
  if ($line -match '\bDisc\b') {
    $system = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    Invoke-AsTask 'AnvilWinvmTscon' "tscon.exe $id /dest:console" $system | Out-Null
    Start-Sleep -Seconds 2
  }

  $out = Join-Path $env:TEMP "winvm-session-$PID.txt"
  $argsList = @("-Action $Action", "-InSession", "-OutFile '$out'")
  foreach ($p in 'Exe', 'WorkDir', 'Path', 'Text', 'Name') {
    $v = Get-Variable $p -ValueOnly
    if ($v) { $argsList += "-$p '$($v -replace "'", "''")'" }
  }
  if ($ArgList.Count) { $argsList += '-ArgList ' + (($ArgList | ForEach-Object { "'$($_ -replace "'", "''")'" }) -join ',') }
  if ($PSBoundParameters.ContainsKey('X')) { $argsList += "-X $X -Y $Y -Button $Button" }
  if ($PSBoundParameters.ContainsKey('Width')) { $argsList += "-Width $Width -Height $Height" }
  $cmd = "& '$PSCommandPath' $($argsList -join ' ')"
  $interactive = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
  $result = Invoke-AsTask "AnvilWinvm-$PID" $cmd $interactive
  if (Test-Path $out) { Get-Content $out; Remove-Item $out }
  exit $result
}

# From here on we are in the interactive session.
try {
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class WinvmUser32 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr p);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct DEVMODE {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
    public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra; public int dmFields;
    public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
    public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
    public short dmLogPixels; public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
    public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
  }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool EnumDisplaySettings(string d, int mode, ref DEVMODE dm);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int ChangeDisplaySettings(ref DEVMODE dm, int flags);
}
'@
  # Physical pixels, so screenshots and click positions agree at any display scaling.
  [WinvmUser32]::SetProcessDPIAware() | Out-Null

  $result = switch ($Action) {
    'launch' {
      $start = @{ FilePath = $Exe; WorkingDirectory = $WorkDir; PassThru = $true }
      if ($ArgList.Count) { $start.ArgumentList = $ArgList }
      $p = Start-Process @start
      Start-Sleep -Seconds 3
      if ($p.HasExited) { "exited at once with code $($p.ExitCode)" } else { "started pid $($p.Id)" }
    }
    'shot' {
      $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
      $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
      $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
      $g.Dispose(); $bmp.Dispose()
      "$($b.Width)x$($b.Height)"
    }
    'click' {
      [WinvmUser32]::SetCursorPos($X, $Y) | Out-Null
      $down, $up = if ($Button -eq 'right') { 0x8, 0x10 } else { 0x2, 0x4 }
      [WinvmUser32]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero)
      Start-Sleep -Milliseconds 50
      [WinvmUser32]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
      "clicked $Button at $X,$Y"
    }
    'type' {
      # Brace the characters SendKeys treats as commands so the text is typed literally.
      $escaped = [regex]::Replace($Text, '[+^%~(){}\[\]]', { param($m) '{' + $m.Value + '}' })
      [System.Windows.Forms.SendKeys]::SendWait($escaped)
      "typed $($Text.Length) characters"
    }
    'key' { [System.Windows.Forms.SendKeys]::SendWait($Text); "sent $Text" }
    'windows' {
      $lines = New-Object System.Collections.Generic.List[string]
      $cb = [WinvmUser32+EnumProc] {
        param($h, $p)
        if ([WinvmUser32]::IsWindowVisible($h)) {
          $sb = New-Object System.Text.StringBuilder 512
          [WinvmUser32]::GetWindowText($h, $sb, 512) | Out-Null
          if ($sb.Length) {
            $r = New-Object WinvmUser32+RECT
            [WinvmUser32]::GetWindowRect($h, [ref]$r) | Out-Null
            $procId = 0; [WinvmUser32]::GetWindowThreadProcessId($h, [ref]$procId) | Out-Null
            $proc = (Get-Process -Id $procId -ErrorAction SilentlyContinue).ProcessName
            $lines.Add("$proc`t[$($r.Left),$($r.Top),$($r.Right),$($r.Bottom)]`t$sb")
          }
        }
        $true
      }
      [WinvmUser32]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
      $lines -join "`n"
    }
    'resolution' {
      $dm = New-Object WinvmUser32+DEVMODE
      $dm.dmSize = [System.Runtime.InteropServices.Marshal]::SizeOf($dm)
      [WinvmUser32]::EnumDisplaySettings($null, -1, [ref]$dm) | Out-Null
      $dm.dmPelsWidth = $Width; $dm.dmPelsHeight = $Height; $dm.dmFields = 0x180000
      # CDS_UPDATEREGISTRY, so the size survives a reboot.
      $r = [WinvmUser32]::ChangeDisplaySettings([ref]$dm, 1)
      if ($r -ne 0) { throw "ChangeDisplaySettings returned $r (the display driver may not offer ${Width}x$Height)" }
      "display set to ${Width}x$Height"
    }
    'close' {
      $procs = Get-Process -Name $Name -ErrorAction SilentlyContinue
      $procs | ForEach-Object { $_.CloseMainWindow() | Out-Null }
      Start-Sleep -Seconds 5
      $procs | Where-Object { -not $_.HasExited } | Stop-Process -Force
      "closed $($procs.Count) $Name process(es)"
    }
  }
  Set-Content -Path $OutFile -Value $result
  exit 0
} catch {
  Set-Content -Path $OutFile -Value "error: $_"
  exit 1
}
