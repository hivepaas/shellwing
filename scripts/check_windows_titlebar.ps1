# What Windows is told about the title bar this app draws itself, and whether
# its three caption buttons work.
#
# GPUI answers `WM_NCHITTEST` with the first `WindowControlArea` whose hitbox is
# under the pointer, and that list is in paint order, so a parent comes before
# its children. `Drag` becomes `HTCAPTION`, and Windows then carries out the
# press itself: the window never gets `WM_LBUTTONDOWN`. A bar marked `Drag`
# therefore kills the caption buttons painted inside it -- unless they occlude
# the mouse, which the appearance switch does and they do not.
#
# **Nothing fails loudly when this is wrong.** The bar still drags, so it reads
# as a broken button rather than as hit testing, and no test on Linux or macOS
# can see it: `windows` is `cfg!(target_os = "windows")`, so that branch
# compiles everywhere and runs only here. It has been met in all three apps.
#
# **It must run in an interactive desktop session.** It moves the real pointer,
# and GPUI answers from the hitboxes under the real pointer, so a hit test
# asked at coordinates the cursor is not at reports the wrong place. An ssh
# session is a network logon with no input desktop; run it through a task:
#
#   schtasks /Create /TN titlebar /SC ONCE /ST 00:00 /IT /F /TR `
#     "powershell -NoProfile -File C:\...\check_windows_titlebar.ps1 C:\...\shellwing.exe"
#   schtasks /Run /TN titlebar
#
# Interactively it is just:
#
#   powershell -NoProfile -File scripts/check_windows_titlebar.ps1 target/release/shellwing.exe
#
# Exits 0 when the buttons work, 1 when they do not, and 2 when it could not be
# run here -- which is not a pass, and must not be read as one.
#
# **A verdict about the app always carries a marker**: `::error::` for a
# failure, and "the title bar answers for its three buttons" for a pass.
# PowerShell exits 1 for its own errors too, so an exit of 1 with neither
# marker is this script being broken rather than an answer about the app. The
# first run of it exited 1 on a parse error, which read exactly like the bug it
# was written to find. Parse it before trusting a verdict:
#
#   [System.Management.Automation.Language.Parser]::ParseFile($p, [ref]$null, [ref]$errs)

param([Parameter(Mandatory)][string]$Exe)

$ErrorActionPreference = 'Stop'
$Exe = (Resolve-Path $Exe).Path

# `CAPTION_W` in the app's crates/app/src/chrome.rs, in logical pixels.
$CaptionW = 46

function Fail([string]$why) {
    Write-Host "::error::$why"
    exit 1
}

function Cannot([string]$why) {
    Write-Host "::warning::cannot check the title bar here: $why"
    Write-Host 'This is not a pass. The buttons are unmeasured.'
    exit 2
}

Add-Type @'
using System;
using System.Runtime.InteropServices;
public class TitleBar {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@

$named = @{
    0 = 'HTNOWHERE'; 1 = 'HTCLIENT'; 2 = 'HTCAPTION'; 3 = 'HTSYSMENU'; 8 = 'HTMINBUTTON'
    9 = 'HTMAXBUTTON'; 20 = 'HTCLOSE'; 10 = 'HTLEFT'; 11 = 'HTRIGHT'; 12 = 'HTTOP'
    13 = 'HTTOPLEFT'; 14 = 'HTTOPRIGHT'; 15 = 'HTBOTTOM'; 18 = 'HTBORDER'; -1 = 'HTTRANSPARENT'
}
function NameOf([int]$c) { if ($named.ContainsKey($c)) { $named[$c] } else { "code $c" } }

# The pointer goes there first: GPUI answers for where the pointer is, not for
# the point asked about.
function HitAt([IntPtr]$h, [int]$x, [int]$y) {
    [void][TitleBar]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds 70
    $packed = [int64]((($y -band 0xFFFF) -shl 16) -bor ($x -band 0xFFFF))
    [int][TitleBar]::SendMessageW($h, 0x0084, [IntPtr]::Zero, [IntPtr]$packed)
}

# Runs of one answer along a row, left to right.
function Sweep([IntPtr]$h, [int]$y, [int]$from, [int]$to, [int]$step) {
    $bands = New-Object System.Collections.ArrayList
    $last = $null
    for ($x = $from; $x -le $to; $x += $step) {
        $c = HitAt $h $x $y
        if ($null -eq $last -or $last.code -ne $c) {
            $last = [pscustomobject]@{ code = $c; first = $x; last = $x }
            [void]$bands.Add($last)
        } else {
            $last.last = $x
        }
    }
    $bands
}

Write-Host "exe: $Exe"
$app = Start-Process -FilePath $Exe -PassThru
try {
    $h = [IntPtr]::Zero
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 500
        $app.Refresh()
        if ($app.HasExited) { Cannot "the program ended with $($app.ExitCode) instead of opening a window" }
        if ($app.MainWindowHandle -ne [IntPtr]::Zero) { $h = $app.MainWindowHandle; break }
    }
    if ($h -eq [IntPtr]::Zero) {
        Cannot 'no window after 30 seconds (on a VM, the renderer may have no display adapter it accepts)'
    }
    [void][TitleBar]::SetForegroundWindow($h)

    $r = New-Object TitleBar+RECT
    [void][TitleBar]::GetWindowRect($h, [ref]$r)
    $dpi = [TitleBar]::GetDpiForWindow($h)
    $scale = $dpi / 96.0
    Write-Host "window: L=$($r.Left) T=$($r.Top) R=$($r.Right) B=$($r.Bottom) dpi=$dpi"

    # Can this session move the pointer at all? Over ssh it cannot, and every
    # answer below would then be about wherever the pointer really is.
    $probe = [TitleBar+POINT]::new()
    $want = @{ x = [int](($r.Left + $r.Right) / 2); y = [int](($r.Top + $r.Bottom) / 2) }
    [void][TitleBar]::SetCursorPos($want.x, $want.y)
    [void][TitleBar]::GetCursorPos([ref]$probe)
    if ([Math]::Abs($probe.X - $want.x) -gt 2 -or [Math]::Abs($probe.Y - $want.y) -gt 2) {
        Cannot 'the pointer does not move, so this is not the input desktop (over ssh, use a scheduled task with /IT)'
    }

    # The window needs a moment before its hitboxes settle: measured, a sweep
    # 800 ms after raising it alternated answers and was clean a second later.
    Start-Sleep -Milliseconds 2500

    $step = [int][Math]::Max(1, [Math]::Round(6 * $scale))
    $from = [int]($r.Right - [Math]::Round(($CaptionW * 8) * $scale))
    $to = $r.Right - 1
    # The bar's own top is not the window's when the window carries an
    # invisible resize frame, so try a few rows down from it and take the first
    # that answers like a whole title bar: the three buttons *and* somewhere to
    # drag by. Measured: the topmost rows give the buttons but answer HTCLIENT
    # beside them, so a row chosen on the buttons alone fails the drag check
    # for a reason of its own.
    $rows = foreach ($down in 10, 18, 22, 26, 30, 34, 42) { [int]($r.Top + [Math]::Round($down * $scale)) }

    $wanted = @(8, 9, 20)
    $found = $null
    $buttonsOnly = $null
    foreach ($y in $rows) {
        $bands = Sweep $h $y $from $to $step
        Write-Host ''
        Write-Host "--- y=$y, x $from..$to step $step ---"
        foreach ($b in $bands) {
            Write-Host ("  x {0,6}..{1,6}  {2,-12} ({3})" -f $b.first, $b.last, (NameOf $b.code), $b.code)
        }
        $controls = @($bands | Where-Object { $wanted -contains $_.code })
        $ordered = $controls.Count -eq 3 -and (($controls | ForEach-Object { $_.code }) -join ',') -eq '8,9,20'
        if (-not $ordered) { continue }
        $row = @{ y = $y; bands = $bands; controls = $controls }
        # A band of HTCAPTION at least a button wide, not a stray point of it:
        # the claim is that the window can still be dragged by the bar.
        $drag = $bands | Where-Object { $_.code -eq 2 } |
            ForEach-Object { $_.last - $_.first + $step } | Sort-Object -Descending | Select-Object -First 1
        if ($drag -ge [Math]::Round($CaptionW * $scale)) { $found = $row; break }
        if ($null -eq $buttonsOnly) { $buttonsOnly = $row }
    }

    if ($null -eq $found -and $null -ne $buttonsOnly) {
        Fail ('the three buttons answer, but no row that gives them has an HTCAPTION band a button ' +
            'wide beside them: the bar has its buttons and nothing to drag the window by')
    }
    if ($null -eq $found) {
        Fail ('no row of the title bar answers HTMINBUTTON, HTMAXBUTTON and HTCLOSE in that order: ' +
            'the caption buttons are inside a drag area, so Windows takes the whole bar for the caption ' +
            'and a press on them does nothing. The bar must carry no window control area; mark the drag ' +
            'areas inside it and leave the buttons outside them.')
    }

    # Each button is CAPTION_W wide; a band much narrower than that means the
    # answers are not the buttons' own.
    $least = [Math]::Round($CaptionW * 0.6 * $scale)
    foreach ($b in $found.controls) {
        $wide = $b.last - $b.first + $step
        if ($wide -lt $least) {
            Fail ("the $(NameOf $b.code) band is $wide px wide, under ${least}: not a whole caption button")
        }
    }
    # And the press itself, which goes through Windows rather than through any
    # reading of the answers above.
    $min = $found.controls[0]
    $cx = [int](($min.first + $min.last) / 2)
    Write-Host ''
    Write-Host "clicking the minimise button at x=$cx y=$($found.y)"
    [void][TitleBar]::SetCursorPos($cx, $found.y)
    Start-Sleep -Milliseconds 250
    [TitleBar]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [TitleBar]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 1200
    $iconic = [TitleBar]::IsIconic($h)
    Write-Host "IsIconic: $iconic"
    if (-not $iconic) {
        Fail 'a real click on the minimise button did not minimise the window, although it answers HTMINBUTTON'
    }
    [void][TitleBar]::ShowWindow($h, 9)
    Write-Host ''
    Write-Host 'the title bar answers for its three buttons, and the click works'
} finally {
    if ($null -ne $app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
}
