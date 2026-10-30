<#
.SYNOPSIS
  Release gate (RELEASING.md step 9): the previous version must update itself to the new one.

.DESCRIPTION
  Serves a folder of release assets (a downloaded DRAFT: gh release download v<ver> -D <dir>)
  plus a GitHub-style "latest release" JSON on http://127.0.0.1:<Port>/, and drives the
  previous published version through OPENCIRCLE_TEST_UPDATE_URL, once per install kind:

    setup     the previous OpenCircle-Setup exe installed (/quiet, Program Files), the app run
              as the normal (non-elevated) user; an updater that asks the user ("Install now")
              gets its card clicked, Windows' admin prompt auto-approved for the run
    portable  the previous portable exe run from a temp folder of this user

  PASS per kind = the copy ends on the new version, it runs, and it says its notes were shown
  (the log's "update <ver>: running" line, written when the What's new card opens).
  Every kind prints what the user would have seen (the updater's log lines).

  Run it on a Windows test machine, ELEVATED and in the interactive desktop (it installs, sets
  and restores the admin-prompt policy, and clicks the update card). It leaves the new version
  installed; config, policy, server and temp files are restored / removed.
  The signature check stays: the assets must be signed with the key the PREVIOUS version trusts.

.EXAMPLE
  gh release download v0.1.1 -p "OpenCircle-Setup-*" -p "*-x64-portable.exe" -D prev
  gh release download v0.1.2 -D draft          # the draft, before it is published
  powershell -ExecutionPolicy Bypass -File update-gate.ps1 -Previous prev -New draft
#>
param(
    [Parameter(Mandatory)] [string]$Previous,
    [Parameter(Mandatory)] [string]$New,
    [string]$Notes = "",
    [string[]]$Kinds = @("setup", "portable"),
    [int]$Port = 8765,
    [int]$TimeoutSec = 300,
    [string]$Shots = "",        # a folder: a screenshot at each step
    [switch]$ShowPrompt         # setup kind: show Windows' admin prompt once (for a screenshot), then approve
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class GateWin {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, int d, UIntPtr e);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
[GateWin]::SetProcessDPIAware() | Out-Null

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw "run this elevated (it installs the previous setup)" }

$Previous = (Resolve-Path $Previous).Path
$New = (Resolve-Path $New).Path
function VersionOf($dir) {
    $s = Get-ChildItem $dir -Filter "OpenCircle-Setup-*-x64.exe" | Select-Object -First 1
    if (-not $s) { throw "no OpenCircle-Setup-<ver>-x64.exe in $dir" }
    [pscustomobject]@{ Setup = $s.FullName; Version = ($s.Name -replace '^OpenCircle-Setup-(.*)-x64\.exe$', '$1') }
}
$prev = VersionOf $Previous
$next = VersionOf $New
foreach ($f in "SHA256SUMS.txt", "SHA256SUMS.txt.minisig") {
    if (-not (Test-Path (Join-Path $New $f))) { throw "$f missing in ${New}: the updater refuses an unsigned release" }
}
$prevPortable = Get-ChildItem $Previous -Filter "*-x64-portable.exe" | Select-Object -First 1
"gate: $($prev.Version) -> $($next.Version), kinds: $($Kinds -join ', ')"

$work = Join-Path $env:LOCALAPPDATA "oc-update-gate"  # not %TEMP%: an exe run from there looks like malware
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force $work | Out-Null
$base = "http://127.0.0.1:$Port"
# a plain string: Get-Content -Raw adds PS* properties that bloat the JSON past the app's limit
$body = if ($Notes -and (Test-Path $Notes)) { [IO.File]::ReadAllText((Resolve-Path $Notes).Path) } else { "## Update test`n- served by the release gate" }
$release = [ordered]@{
    tag_name   = "v$($next.Version)"; name = "OpenCircle $($next.Version)"; draft = $false; prerelease = $false
    html_url   = "https://github.com/AmerAlreyahi/OpenCircle/releases/tag/v$($next.Version)"; body = $body
    assets     = @(Get-ChildItem $New -File | ForEach-Object {
        [ordered]@{ name = $_.Name; size = $_.Length; browser_download_url = "$base/$([Uri]::EscapeDataString($_.Name))" } })
}
$json = Join-Path $work "latest.json"
[IO.File]::WriteAllText($json, ($release | ConvertTo-Json -Depth 5))  # no BOM: the app reads plain JSON

# The server: the assets by name (flat), the JSON at /latest.
$server = Start-Job -ArgumentList $New, $json, $Port -ScriptBlock {
    param($root, $json, $port)
    $l = [System.Net.HttpListener]::new(); $l.Prefixes.Add("http://127.0.0.1:$port/"); $l.Start()
    while ($l.IsListening) {
        $c = $l.GetContext()
        $name = [Uri]::UnescapeDataString($c.Request.Url.AbsolutePath.TrimStart('/'))
        $file = if ($name -eq "latest") { $json } elseif ($name -match '^[\w.+-]+$') { Join-Path $root $name } else { "" }
        if ($file -and (Test-Path -LiteralPath $file -PathType Leaf)) {
            $c.Response.ContentLength64 = (Get-Item -LiteralPath $file).Length
            $fs = [IO.File]::OpenRead($file); $fs.CopyTo($c.Response.OutputStream); $fs.Close()
        } else { $c.Response.StatusCode = 404 }
        $c.Response.Close()
    }
}
$url = "$base/latest"

$log = Join-Path $env:LOCALAPPDATA "OpenCircle\logs\opencircle.log"
$config = Join-Path $env:APPDATA "opencircle\config.toml"
$configBackup = Join-Path $work "config.toml"
$policy = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System"
$consent = (Get-ItemProperty $policy).ConsentPromptBehaviorAdmin
$secure = (Get-ItemProperty $policy).PromptOnSecureDesktop
$step = 0

function Shot($name) {
    if (-not $Shots) { return }
    New-Item -ItemType Directory -Force $Shots | Out-Null
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $script:step++
    try {
        $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
        $bmp.Save((Join-Path $Shots ("{0:d2}-{1}.png" -f $script:step, $name)))
    } catch { "  (no screenshot of ${name}: $($_.Exception.Message))" | Out-Host }
    $g.Dispose(); $bmp.Dispose()
}
function LogSince($from) {
    if (-not (Test-Path $log)) { return @() }
    $fs = [IO.File]::Open($log, 'Open', 'Read', 'ReadWrite'); $fs.Seek($from, 'Begin') | Out-Null
    $text = (New-Object IO.StreamReader $fs).ReadToEnd(); $fs.Close()
    @($text -split "`r?`n" | Where-Object { $_ -match "update" })
}
function LogEnd { if (Test-Path $log) { (Get-Item $log).Length } else { 0 } }
function QuitApp {
    $p = @(Get-Process opencircle -ErrorAction SilentlyContinue); if (-not $p) { return }
    try { & $p[0].Path quit 2>&1 | Out-Null } catch {}
    for ($i = 0; $i -lt 50 -and (Get-Process opencircle -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 200 }
    Get-Process opencircle -ErrorAction SilentlyContinue | Stop-Process -Force
}
# As the signed-in user, NOT elevated (a scheduled task's default run level), with the hook set.
function StartAsUser($exe) {
    $cmd = Join-Path $work "start.cmd"
    "@set OPENCIRCLE_TEST_UPDATE_URL=$url`r`n@start `"`" `"$exe`"" | Set-Content -Encoding ASCII $cmd
    schtasks /create /tn oc-update-gate /tr "`"$cmd`"" /sc once /st 23:59 /it /f | Out-Null
    schtasks /run /tn oc-update-gate | Out-Null
}
function Version($exe) { (Get-Item $exe).VersionInfo.FileVersion }
function Running($exe) { @(Get-Process opencircle -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }).Count -gt 0 }
# The update card (a ~480 px wide floating window of the app): its "Install now" button.
function ClickInstallNow {
    $hits = New-Object System.Collections.ArrayList
    $ids = @(Get-Process opencircle -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
    [GateWin]::EnumWindows({ param($h, $l)
        $p = [uint32]0; [GateWin]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
        $r = New-Object GateWin+RECT
        if ($ids -contains $p -and [GateWin]::IsWindowVisible($h) -and [GateWin]::GetWindowRect($h, [ref]$r)) {
            $w = $r.R - $r.L; $hgt = $r.B - $r.T
            if ($w -ge 300 -and $w -le 1500 -and $hgt -ge 150 -and $hgt -le 1400) { [void]$hits.Add($r) }
        }
        $true }, [IntPtr]::Zero) | Out-Null
    if ($hits.Count -eq 0) { return $false }
    $r = $hits[0]; $s = ($r.R - $r.L) / 480.0
    $x = [int]($r.L + 100 * $s); $y = [int]($r.B - 44 * $s)
    [GateWin]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds 300
    [GateWin]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [GateWin]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    $true
}

function RunKind($kind) {
    QuitApp
    $from = LogEnd
    if ($kind -eq "setup") {
        & $prev.Setup /uninstall /quiet | Out-Null  # whatever version is installed goes first
        $p = Start-Process $prev.Setup -ArgumentList "/quiet" -Wait -PassThru
        if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) { return "FAIL (the previous setup did not install: $($p.ExitCode))" }
        QuitApp
        $folder = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\OpenCircle").InstallLocation
        $exe = Join-Path $folder "opencircle.exe"
    } else {
        if (-not $prevPortable) { return "FAIL (no previous portable exe)" }
        $dir = Join-Path $work "portable"; New-Item -ItemType Directory -Force $dir | Out-Null
        $exe = Join-Path $dir "opencircle.exe"; Copy-Item $prevPortable.FullName $exe
    }
    "[$kind] $exe = $(Version $exe)"
    $from = LogEnd
    StartAsUser $exe
    $declined = -not $ShowPrompt
    $carded = $false
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $done = $false
    while ((Get-Date) -lt $deadline -and -not $done) {
        Start-Sleep -Seconds 2
        $lines = LogSince $from
        if ($kind -eq "setup" -and ($lines -match "choose Install now") -and -not ($lines -match "the setup runs")) {
            if (-not $carded) { Shot "$kind-ready-card"; $carded = $true }
            if (-not $declined) {
                Set-ItemProperty $policy ConsentPromptBehaviorAdmin 5   # the prompt shows,
                Set-ItemProperty $policy PromptOnSecureDesktop 0        # on this desktop (a screenshot)
                ClickInstallNow | Out-Null; Start-Sleep -Seconds 4
                Shot "$kind-admin-prompt"
                # Windows ignores synthetic clicks on its prompt: it is closed instead and the
                # app restarted (a real "No" = nothing changes: the unit tests)
                Get-Process consent -ErrorAction SilentlyContinue | Stop-Process -Force
                QuitApp; Set-ItemProperty $policy PromptOnSecureDesktop $secure
                "[$kind] admin prompt shown; still $(Version $exe)"
                $declined = $true; $from = LogEnd; StartAsUser $exe
            } else {
                Set-ItemProperty $policy ConsentPromptBehaviorAdmin 0   # approved without a prompt
                if (ClickInstallNow) { Start-Sleep -Seconds 5 }
            }
        }
        $done = ($lines -match [regex]::Escape("update $($next.Version): running")) -and (Running $exe)
        # a final answer from the updater: no need to wait out the timeout
        if ($lines -match "cannot replace|could not be verified|could not reach|get it from the release page|is up to date") { break }
    }
    Set-ItemProperty $policy ConsentPromptBehaviorAdmin $consent
    Set-ItemProperty $policy PromptOnSecureDesktop $secure
    Start-Sleep -Seconds 3
    Shot "$kind-end"
    "[$kind] what the user saw (log):"
    LogSince $from | ForEach-Object { "    $_" }
    $v = Version $exe
    $ok = $done -and $v -and $v.StartsWith($next.Version)
    QuitApp
    if ($ok) { "PASS" } else { "FAIL (ends on $v, running: $(Running $exe), notes: $done)" }
}

# `key = value` in the TOML table `[section]` (its first lines), replacing one that is there.
function SetKey($text, $section, $key, $value) {
    $lines = [Collections.Generic.List[string]]($text -split "`r?`n")
    $start = $lines.IndexOf("[$section]")
    if ($start -lt 0) { $lines.Add("[$section]"); $start = $lines.Count - 1 }
    for ($i = $start + 1; $i -lt $lines.Count -and -not $lines[$i].StartsWith("["); $i++) {
        if ($lines[$i] -match "^\s*$key\s*=") { $lines.RemoveAt($i); $i-- }
    }
    $lines.Insert($start + 1, "$key = $value")
    ($lines -join "`n")
}

$results = [ordered]@{}
try {
    if (Test-Path $config) { Copy-Item $config $configBackup }
    # Updates on; "Start with Windows" off for the run (a temp-folder exe writing the Run key
    # trips Defender's persistence heuristic); the user's config comes back at the end.
    $text = if (Test-Path $config) { Get-Content -Raw $config } else { "" }
    $text = SetKey $text "update" "auto" "true"
    $text = SetKey $text "general" "autostart" "false"
    New-Item -ItemType Directory -Force (Split-Path $config) | Out-Null
    [IO.File]::WriteAllText($config, $text)
    foreach ($k in $Kinds) {
        $out = RunKind $k
        $out | Select-Object -SkipLast 1 | ForEach-Object { $_ }
        $results[$k] = $out | Select-Object -Last 1
    }
} finally {
    QuitApp
    Set-ItemProperty $policy ConsentPromptBehaviorAdmin $consent
    Set-ItemProperty $policy PromptOnSecureDesktop $secure
    schtasks /delete /tn oc-update-gate /f 2>&1 | Out-Null
    Stop-Job $server -ErrorAction SilentlyContinue; Remove-Job $server -Force -ErrorAction SilentlyContinue
    if (Test-Path $configBackup) { Copy-Item $configBackup $config -Force }
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
""
"RESULT $($prev.Version) -> $($next.Version)"
$results.GetEnumerator() | ForEach-Object { "  {0,-9} {1}" -f $_.Key, $_.Value }
if (@($results.Values | Where-Object { $_ -ne "PASS" }).Count) { exit 1 }
