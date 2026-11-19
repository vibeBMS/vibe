# Runs one stage of the runner probe (README.md) and writes its outputs to
# $Out: logs, screenshots and timing tables, nothing of LR2's.
#   probe.ps1 -Stage env|d3d9|sound|acp|le|input -Probe lr2probe.exe -Out DIR [-Tag TAG]
param(
    [Parameter(Mandatory)] [string] $Stage,
    [Parameter(Mandatory)] [string] $Probe,
    [Parameter(Mandatory)] [string] $Out,
    [string] $Tag = ''
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
$name = if ($Tag) { "$Stage-$Tag" } else { $Stage }

function Shot([string] $file) {
    # one frame of the whole desktop through GDI, as the README asks
    & ffmpeg -hide_banner -loglevel error -y -f gdigrab -framerate 5 -i desktop -frames:v 1 (Join-Path $Out $file)
    if ($LASTEXITCODE) { Write-Host "::warning::gdigrab failed ($LASTEXITCODE)" } else { Write-Host "screenshot $file" }
}

# runs the probe with ARGS in the background, takes a screenshot after
# SHOT_AFTER seconds (0: none) and returns its exit code; its output goes
# to $Out\$log and the job log
function RunProbe([string[]] $probeArgs, [string] $log, [int] $shotAfter = 0, [string] $shot = '', [int] $timeout = 60) {
    $o = Join-Path $Out $log
    $p = Start-Process -FilePath $Probe -ArgumentList $probeArgs -PassThru -NoNewWindow `
        -RedirectStandardOutput $o -RedirectStandardError "$o.err"
    if ($shotAfter -gt 0) { Start-Sleep -Seconds $shotAfter; Shot $shot }
    if (-not $p.WaitForExit($timeout * 1000)) { $p.Kill(); Write-Host "::warning::probe $probeArgs timed out" }
    $p.WaitForExit()
    Get-Content $o, "$o.err" -ErrorAction SilentlyContinue | Write-Host
    Write-Host "exit code $($p.ExitCode)"
    return $p.ExitCode
}

switch ($Stage) {
    'env' {
        $f = Join-Path $Out 'env.txt'
        & {
            Get-CimInstance Win32_OperatingSystem | Format-List Caption, Version, OSArchitecture, OSLanguage, Locale, CodeSet
            Get-CimInstance Win32_VideoController | Format-List Name, DriverVersion, VideoProcessor, CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate, AdapterRAM
            'Sound devices (Win32_SoundDevice):'; Get-CimInstance Win32_SoundDevice | Format-Table -AutoSize Name, Status
            'PnP audio:'; Get-PnpDevice -Class AudioEndpoint, MEDIA -ErrorAction SilentlyContinue | Format-Table -AutoSize Class, FriendlyName, Status
            Get-Service Audiosrv, AudioEndpointBuilder | Format-Table -AutoSize Name, Status, StartType
            "System locale: $((Get-WinSystemLocale).Name)"
            "Culture: $((Get-Culture).Name), UI culture: $((Get-UICulture).Name)"
            "ANSI code page (registry): $((Get-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Control\Nls\CodePage).ACP)"
            "User languages: $((Get-WinUserLanguageList).LanguageTag -join ', ')"
            "UserInteractive: $([Environment]::UserInteractive), session: $((Get-Process -Id $PID).SessionId), user: $env:USERNAME"
            'Sessions:'; (qwinsta 2>&1) -join "`n"
            'Japanese fonts:'; Get-ChildItem C:\Windows\Fonts -Include msgothic*, msmincho*, meiryo*, YuGoth*, *jp* -Recurse -ErrorAction SilentlyContinue | ForEach-Object Name
            'D3D9 and D3DX9 (32-bit):'; Get-ChildItem C:\Windows\SysWOW64 -Filter 'd3d*9*.dll' | ForEach-Object { "$($_.Name) $($_.VersionInfo.FileVersion)" }
            "ffmpeg: $((Get-Command ffmpeg -ErrorAction SilentlyContinue).Source)"
        } *>&1 | Out-String -Width 200 | Tee-Object -FilePath $f | Write-Host
        Shot 'desktop.png'
    }
    'd3d9' {
        $fs = if ($Tag -eq 'fullscreen') { @('fullscreen') } else { @() }
        $code = RunProbe (@('d3d9', '8') + $fs) "$name.txt" 4 "$name.png"
        exit $code
    }
    'sound' { exit (RunProbe @('sound') "$name.txt") }
    'acp' { exit (RunProbe @('acp') "$name.txt") }
    'le' {
        # Locale Emulator (open source, LGPL) runs the probe as if the
        # system locale were Japanese: `LEProc.exe PATH ARGS` is its default
        # ja-JP profile
        $le = Join-Path $env:RUNNER_TEMP 'le'
        New-Item -ItemType Directory -Force $le | Out-Null
        & gh release download --repo xupefei/Locale-Emulator --pattern '*.zip' --dir $le --clobber
        if ($LASTEXITCODE) { exit 1 }
        Get-ChildItem $le -Filter *.zip | ForEach-Object { Write-Host "Locale Emulator $($_.Name)"; Expand-Archive $_.FullName -DestinationPath $le -Force }
        $leproc = Get-ChildItem $le -Recurse -Filter LEProc.exe | Select-Object -First 1
        # LEProc needs LECommonLibrary.dll, which only LEInstaller (a window
        # with an Install button) writes out, from its resources: written
        # here from the same resource, without the window
        $dll = Join-Path $leproc.DirectoryName 'LECommonLibrary.dll'
        if (-not (Test-Path $dll)) {
            $cmd = "`$a = [Reflection.Assembly]::LoadFile('$(Join-Path $leproc.DirectoryName 'LEInstaller.exe')'); " +
                "`$n = `$a.GetManifestResourceNames() | Where-Object { `$_ -like '*.Resources.resources' } | Select-Object -First 1; " +
                "`$rm = New-Object Resources.ResourceManager(`$n -replace '\.resources$', ''), `$a; " +
                "[IO.File]::WriteAllBytes('$dll', [byte[]] `$rm.GetObject('LECommonLibrary'))"
            & powershell.exe -NoProfile -Command $cmd
            Write-Host "LECommonLibrary.dll from LEInstaller: $((Get-Item $dll -ErrorAction SilentlyContinue).Length) bytes"
        }
        $res = Join-Path $Out 'le-acp.txt'
        $start = Get-Date
        $p = Start-Process -FilePath $leproc.FullName -ArgumentList @("`"$Probe`"", 'acp', "`"$res`"") -PassThru
        $p.WaitForExit(30000) | Out-Null
        Write-Host "LEProc exit code $($p.ExitCode)"
        if ($p.ExitCode) {
            # a crash leaves its exception in the Application log
            Get-WinEvent -ErrorAction SilentlyContinue -FilterHashtable @{
                LogName = 'Application'; ProviderName = '.NET Runtime', 'Application Error'; StartTime = $start
            } | Select-Object -First 3 | ForEach-Object { $_.Message } |
                Tee-Object -FilePath (Join-Path $Out 'le-crash.txt') | Write-Host
            # the same entry point under Windows PowerShell's .NET Framework,
            # for the exception's message (the log has only its type)
            $cmd = "try { `$a = [Reflection.Assembly]::LoadFile('$($leproc.FullName)'); " +
                "`$a.EntryPoint.Invoke(`$null, @(,[string[]]@('$Probe', 'acp', '$res'))) } " +
                "catch { `$e = `$_.Exception; while (`$e) { `$e.GetType().FullName + ': ' + `$e.Message; if (`$e.FileName) { 'file: ' + `$e.FileName }; `$e = `$e.InnerException } }"
            & powershell.exe -NoProfile -Command $cmd | Tee-Object -Append -FilePath (Join-Path $Out 'le-crash.txt') | Write-Host
        }
        for ($i = 0; $i -lt 15 -and -not (Test-Path $res); $i++) { Start-Sleep -Seconds 1 }
        Start-Sleep -Seconds 1
        Shot 'le.png'
        if (Test-Path $res) { Get-Content $res | Write-Host } else { Write-Host '::warning::no output from the probe under Locale Emulator'; exit 1 }
        if (-not (Select-String -Quiet -Path $res -Pattern 'GetACP: 932')) { exit 1 }
    }
    'input' {
        # 200 presses of Z, 100 ms apart (1/16 notes at BPM 150), into a
        # window reading the keyboard by message and by DirectInput
        $recv = Join-Path $Out "$name-recv.csv"
        $send = Join-Path $Out "$name-send.csv"
        # -Tag d3d9: the window draws with D3D9 and polls once a frame, as a game
        $render = if ($Tag -eq 'd3d9') { @('d3d9') } else { @() }
        $r = Start-Process -FilePath $Probe -ArgumentList (@('recv', '200', "`"$recv`"") + $render) -PassThru -NoNewWindow `
            -RedirectStandardOutput (Join-Path $Out "$name-recv.txt")
        Start-Sleep -Seconds 1
        & $Probe send 200 100 $send
        $r.WaitForExit(60000) | Out-Null
        Get-Content (Join-Path $Out "$name-recv.txt") | Write-Host
        & python (Join-Path $PSScriptRoot 'jitter.py') $send $recv (Join-Path $Out "$name.json") |
            Tee-Object -FilePath (Join-Path $Out "$name.txt") | Write-Host
        exit $LASTEXITCODE
    }
    default { throw "unknown stage $Stage" }
}
