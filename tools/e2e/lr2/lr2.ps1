# Runs LR2 as a black box on the runner: starts it, drives its windows the
# way a person does (clicks, keys, a folder picked in a dialog), reads its
# screen (gdigrab) and the files it writes. LR2 itself stays in
# $env:RUNNER_TEMP\lr2; $Out gets only this script's logs, screenshots and
# numbers.
#   lr2.ps1 -Stage setup|autoplay|loop|play|calibrate|compare -Zip LR2.zip -Probe lr2probe.exe -Out DIR [-Chart NAME] [-Kind c|f]
#     setup      unzip, a player, windowed, vsync off, the charts' folder;
#                LR2 started once (its song list), the select screen
#     autoplay   LR2body.exe -A -NS on a chart, screenshots
#     loop       our own click's latency through VB-CABLE
#     play       a chart played by lr2probe's key schedule, started by the
#                chart's BGM click; the result screen; LR2's score rows
#     calibrate  the latency from the two sweeps of -Kind
#     compare    LR2's counts against vibeBMS's rows
param(
    [Parameter(Mandatory)] [string] $Stage,
    [string] $Zip = '',
    [Parameter(Mandatory)] [string] $Probe,
    [Parameter(Mandatory)] [string] $Out,
    [string] $Chart = '',
    [string] $Kind = 'c'
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8  # the probe writes UTF-8
New-Item -ItemType Directory -Force $Out | Out-Null
$Lr2 = Join-Path $env:RUNNER_TEMP 'lr2'
$Root = Join-Path $Lr2 'LR2beta3'
# the song folder: on the desktop, the top of Browse for Folder's tree
$Songs = Join-Path ([Environment]::GetFolderPath('Desktop')) 'lr2songs'
$Charts = Join-Path $Songs 'e2e'
$LatFile = Join-Path $env:RUNNER_TEMP 'lr2-latency.txt'
$ScoreDb = Join-Path $Root 'LR2files\Database\Score\e2etest.db'
$Py = (Get-Command python).Source
$E2e = Join-Path $PSScriptRoot 'e2e.py'
$Title = 'LR2 beta3 version 100201'

function Shot([string] $file) {
    & ffmpeg -hide_banner -loglevel error -y -f gdigrab -framerate 5 -i desktop -frames:v 1 (Join-Path $Out $file) 2>&1 | Write-Host
}

function Log([string] $file, [scriptblock] $what) {
    & $what *>&1 | Out-String -Width 250 | Tee-Object -Append -FilePath (Join-Path $Out $file) | Write-Host
}

function LE { & (Join-Path $PSScriptRoot 'le.ps1') | Select-Object -Last 1 }

# starts an LR2 program under Locale Emulator (its text is Shift-JIS)
function Start-LR2([string] $exe, [string[]] $lr2Args = @()) {
    $a = @("`"$(Join-Path $Root $exe)`"") + ($lr2Args | ForEach-Object { "`"$_`"" })
    Start-Process -FilePath (LE) -ArgumentList $a -WorkingDirectory $Root | Out-Null
}

function Stop-LR2 {
    Get-Process -Name LR2body, LR2 -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 500
}

# LR2's dialogs other than the setup window: logged with their text, then
# closed with Enter (their default button)
function Popups([string] $tag) {
    Start-Sleep -Seconds 2
    $ids = @(Get-Process -Name LR2, LR2body -ErrorAction SilentlyContinue | ForEach-Object Id)
    $wins = @(& $Probe windows | Where-Object { $_ -match '^(0x[0-9A-Fa-f]+) pid (\d+) \[#32770\] "(.*)"' -and $ids -contains [int]$Matches[2] -and $Matches[3] -ne 'LR2 SETUP' })
    foreach ($w in $wins) {
        $null = $w -match '^(0x[0-9A-Fa-f]+)'
        $h = $Matches[1]
        Log 'setup.txt' { "--- $tag`: $w"; & $Probe ui $h dump | Where-Object { $_ -match '\[Static\]' } }
        & $Probe ui $h key 0D | Out-Null
        Start-Sleep -Milliseconds 500
    }
}

function Tab([string] $name) {
    $x = @{ main = 36; jukebox1 = 91; jukebox2 = 163; option = 224; select = 277; system = 333 }[$name]
    & $Probe ui 'LR2 SETUP' mouse $x 74 | Out-Null
    Popups "tab $name"
}

function Latency {
    if (Test-Path $LatFile) { [double](Get-Content $LatFile) } else { throw 'no latency yet (stage loop)' }
}

switch ($Stage) {
    'setup' {
        if (-not (Test-Path $Root)) { Expand-Archive $Zip -DestinationPath $Lr2 }
        & $Py -I $E2e gen $Charts
        Start-LR2 'LR2.exe'
        Start-Sleep -Seconds 4
        # a player, e2etest / e2e
        & $Probe ui 'LR2 SETUP' click p/1003
        Start-Sleep -Seconds 1
        & $Probe ui 'NEW PLAYER' text 1002 e2etest
        & $Probe ui 'NEW PLAYER' text 1008 e2e
        & $Probe ui 'NEW PLAYER' click 1
        Popups 'new player'
        & $Probe ui 'LR2 SETUP' select p/1001 e2etest | Out-Null
        & $Probe ui 'LR2 SETUP' text p/1002 e2e
        # windowed (gdigrab sees no exclusive fullscreen)
        & $Probe ui 'LR2 SETUP' check p/1006 1
        # the song folder: Browse for Folder, the folder's name typed
        Tab 'jukebox1'
        & $Probe ui 'LR2 SETUP' click p/1035
        Start-Sleep -Seconds 2
        & $Probe ui 'Browse for Folder' type lr2songs
        Start-Sleep -Milliseconds 500
        & $Probe ui 'Browse for Folder' click 1
        Popups 'add folder'
        $n = & $Probe ui 'LR2 SETUP' msg p/1075 1105 0 0  # TVM_GETCOUNT
        Log 'setup.txt' { "folders in JUKEBOX1's tree: $n" }
        Shot 'setup-jukebox.png'
        if ([int]$n -lt 1) { throw 'the song folder was not registered' }
        # vsync off (the lr2-measure README)
        Tab 'system'
        & $Probe ui 'LR2 SETUP' check p/1016 0
        Shot 'setup-system.png'
        Tab 'main'
        # LR2 started: it writes config.xml and reads the song folder
        & $Probe ui 'LR2 SETUP' click 1003
        Start-Sleep -Seconds 15
        Shot 'title.png'
        & $Probe ui $Title key 0D | Out-Null
        Start-Sleep -Seconds 5
        Shot 'select.png'
        & $Probe ui $Title key 28 | Out-Null  # down: the next song
        Start-Sleep -Seconds 2
        Shot 'select-2.png'
        Log 'setup.txt' { '--- windows'; & $Probe windows | Where-Object { $_ -match 'LR2|#32770' } }
        Stop-LR2
        # what LR2 wrote: config.xml's element names (not its values, but
        # the two this stage set)
        $cfg = Join-Path $Root 'LR2files\Config\config.xml'
        $text = [Text.Encoding]::GetEncoding(932).GetString([IO.File]::ReadAllBytes($cfg))
        Log 'config.txt' {
            '--- config.xml elements'
            ([regex]::Matches($text, '<([A-Za-z_][A-Za-z0-9_]*)>') | ForEach-Object { $_.Groups[1].Value } | Select-Object -Unique) -join ' '
            '--- set here'
            foreach ($k in 'screenmode', 'vsync') { if ($text -match "<$k>([^<]*)</$k>") { "$k = $($Matches[1])" } }
        }
        Log 'setup.txt' { '--- score database'; & $Py -I -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); [print(r[0], r[1]) for r in c.execute('select name, sql from sqlite_master')]" $ScoreDb }
    }
    'autoplay' {
        $c = Join-Path $Charts "$Chart.bme"
        Start-LR2 'LR2body.exe' @('-A', '-NS', $c)
        for ($t = 2; $t -le 60; $t += 2) {
            Start-Sleep -Seconds 2
            if (-not (Get-Process -Name LR2body -ErrorAction SilentlyContinue)) { Log 'autoplay.txt' { "LR2 exited after about $t s" }; break }
            Shot ("autoplay-{0}-{1:d2}s.png" -f $Chart, $t)
        }
        Stop-LR2
    }
    'loop' {
        Log 'loop.txt' { & $Probe loop (Join-Path $Out 'loop.csv') }
        $vals = @(Import-Csv (Join-Path $Out 'loop.csv') | ForEach-Object { [double]$_.latency_ms } | Sort-Object)
        if (-not $vals.Count) { throw 'the click was not heard' }
        $median = $vals[[int]($vals.Count / 2)]
        # LR2's output adds its own buffer to ours: a first guess, which the
        # sweeps correct
        $guess = $median + 40
        Log 'loop.txt' { "our click: median $median ms; first guess for LR2: $guess ms" }
        "$guess" | Out-File -Encoding ascii $LatFile
    }
    'play' {
        $lat = Latency
        $c = Join-Path $Charts "$Chart.bme"
        $sched = Join-Path $env:RUNNER_TEMP "$Chart.csv"
        & $Py -I $E2e plan $c $sched
        $log = Join-Path $Out "play-$Chart.csv"
        $p = Start-Process -FilePath $Probe -ArgumentList @('play', "`"$sched`"", '1600', "$lat", "`"$log`"") -PassThru -NoNewWindow `
            -RedirectStandardOutput (Join-Path $Out "play-$Chart.txt")
        Start-LR2 'LR2body.exe' @($c)
        $shot = 0
        while (-not $p.HasExited) {
            Start-Sleep -Seconds 1
            # the play screen, once the presses run
            if ($shot -lt 2 -and (Test-Path $log) -and @(Get-Content $log).Count -gt 8) {
                Shot "play-$Chart-$shot.png"; $shot++; Start-Sleep -Seconds 3
            }
        }
        Get-Content (Join-Path $Out "play-$Chart.txt") | Write-Host
        # the result screen
        Start-Sleep -Seconds 4
        Shot "result-$Chart.png"
        for ($i = 0; $i -lt 10 -and (Get-Process -Name LR2body -ErrorAction SilentlyContinue); $i++) {
            & $Probe ui $Title key 0D | Out-Null
            Start-Sleep -Seconds 2
        }
        Stop-LR2
        & $Py -I $E2e scores $ScoreDb $Charts (Join-Path $Out 'scores.json') | Tee-Object -FilePath (Join-Path $Out "scores-after-$Chart.txt") | Write-Host
    }
    'calibrate' {
        $lat = Latency
        $r = & $Py -I $E2e calibrate (Join-Path $Out 'scores.json') $Kind
        Log 'calibrate.txt' { "latency in use: $lat ms"; $r }
        $j = $r | ConvertFrom-Json
        if (-not $j.valid) { throw "sweep $Kind out of range" }
        $new = $lat + $j.x_ms
        Log 'calibrate.txt' { "latency now: $new ms" }
        "$new" | Out-File -Encoding ascii $LatFile
    }
    'compare' {
        & $Py -I $E2e compare (Join-Path $Out 'scores.json') (Join-Path $Out 'compare.md')
        $code = $LASTEXITCODE
        Get-Content (Join-Path $Out 'compare.md') | Out-File -Append -Encoding utf8 $env:GITHUB_STEP_SUMMARY
        exit $code
    }
    default { throw "unknown stage $Stage" }
}
