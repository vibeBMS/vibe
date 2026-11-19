# Runs LR2 as a black box on the runner: starts it, reads its screen
# (gdigrab) and the files it writes. LR2's own files stay in
# $env:RUNNER_TEMP\lr2; $Out gets only this script's logs and screenshots.
#   lr2.ps1 -Stage explore|setup -Zip LR2.zip -Probe lr2probe.exe -Out DIR
#     explore: LR2body.exe and LR2.exe started as they come
#     setup: LR2.exe (its setup dialog) under Locale Emulator, its controls
#            read through UI Automation
param(
    [Parameter(Mandatory)] [string] $Stage,
    [Parameter(Mandatory)] [string] $Zip,
    [Parameter(Mandatory)] [string] $Probe,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8  # the probe writes UTF-8
New-Item -ItemType Directory -Force $Out | Out-Null
$Lr2 = Join-Path $env:RUNNER_TEMP 'lr2'

function Shot([string] $file) {
    & ffmpeg -hide_banner -loglevel error -y -f gdigrab -framerate 5 -i desktop -frames:v 1 (Join-Path $Out $file) 2>&1 | Write-Host
    Write-Host "screenshot $file"
}

function Log([string] $file, [scriptblock] $what) {
    & $what *>&1 | Out-String -Width 250 | Tee-Object -Append -FilePath (Join-Path $Out $file) | Write-Host
}

function Unpack {
    if (-not (Test-Path $Lr2)) {
        Expand-Archive $Zip -DestinationPath $Lr2
    }
    $body = Get-ChildItem $Lr2 -Recurse -Filter LR2body.exe | Select-Object -First 1
    if (-not $body) { throw 'no LR2body.exe in the zip' }
    return $body.DirectoryName
}

# path, size and time of every file under DIR, to see what LR2 writes
function Snapshot([string] $dir) {
    $h = @{}
    Get-ChildItem $dir -Recurse -File | ForEach-Object { $h[$_.FullName.Substring($dir.Length + 1)] = "$($_.Length) $($_.LastWriteTimeUtc.Ticks)" }
    return $h
}

function Changes([hashtable] $before, [hashtable] $after) {
    foreach ($k in $after.Keys | Sort-Object) {
        if (-not $before.ContainsKey($k)) { "new      $k ($($after[$k].Split(' ')[0]) bytes)" }
        elseif ($before[$k] -ne $after[$k]) { "changed  $k ($($after[$k].Split(' ')[0]) bytes)" }
    }
}

# the element and attribute names of an XML file LR2 wrote, not its values
# (a value shows only as number, text or empty)
function XmlNames([string] $path) {
    $raw = [IO.File]::ReadAllBytes($path)
    $text = [Text.Encoding]::GetEncoding(932).GetString($raw)
    $text = $text -replace '^<\?xml[^>]*\?>', ''
    $doc = New-Object Xml.XmlDocument
    $doc.LoadXml($text)
    function Walk($node, [string] $path) {
        foreach ($c in $node.ChildNodes) {
            if ($c.NodeType -ne 'Element') { continue }
            $p = "$path/$($c.Name)"
            $attrs = ($c.Attributes | ForEach-Object { "@$($_.Name)" }) -join ' '
            $kind = if ($c.HasChildNodes -and $c.ChildNodes.Count -eq 1 -and $c.FirstChild.NodeType -eq 'Text') {
                if ($c.InnerText -match '^-?[0-9.]+$') { 'number' } else { "text($($c.InnerText.Length))" }
            } elseif (-not $c.HasChildNodes) { 'empty' } else { '' }
            "$p $attrs $kind".TrimEnd()
            Walk $c $p
        }
    }
    Walk $doc ''
}

# starts EXE (relative to LR2's folder) with ARGS, takes screenshots at the
# given seconds, lists the windows, then closes it
function Watch([string] $root, [string] $exe, [string[]] $exeArgs, [int[]] $shots, [string] $tag) {
    $p = Start-Process -FilePath (Join-Path $root $exe) -ArgumentList $exeArgs -WorkingDirectory $root -PassThru
    $t0 = Get-Date
    foreach ($s in $shots) {
        $wait = $s - ((Get-Date) - $t0).TotalSeconds
        if ($wait -gt 0) { Start-Sleep -Milliseconds ([int]($wait * 1000)) }
        Shot ("{0}-{1:d2}s.png" -f $tag, $s)
        Log "$tag.txt" { "--- windows at $s s (LR2 pid $($p.Id), exited: $($p.HasExited))"; & $Probe windows }
        if ($p.HasExited) { break }
    }
    if (-not $p.HasExited) {
        $p.CloseMainWindow() | Out-Null
        if (-not $p.WaitForExit(5000)) { Stop-Process -Id $p.Id -Force }
    }
    Get-Process -Name LR2*, ([IO.Path]::GetFileNameWithoutExtension($exe)) -ErrorAction SilentlyContinue | Stop-Process -Force
    Log "$tag.txt" { "exit code: $($p.ExitCode)" }
}

switch ($Stage) {
    'explore' {
        $root = Unpack
        Log 'lr2-files.txt' {
            "LR2 folder: $($root.Substring($Lr2.Length))"
            Get-ChildItem $Lr2 -Recurse -File | ForEach-Object { "$($_.FullName.Substring($Lr2.Length + 1)) $($_.Length)" }
        }
        foreach ($run in @(@{ exe = 'LR2body.exe'; tag = 'body' }, @{ exe = 'LR2.exe'; tag = 'launcher' })) {
            if (-not (Test-Path (Join-Path $root $run.exe))) { Write-Host "no $($run.exe)"; continue }
            $before = Snapshot $root
            Watch $root $run.exe @() @(2, 5, 10, 20, 30) $run.tag
            Log "$($run.tag).txt" { '--- files written'; Changes $before (Snapshot $root) }
        }
        Get-ChildItem $root -Recurse -Filter *.xml | Where-Object { $_.FullName -match 'Config' } | ForEach-Object {
            $f = $_
            Log 'config-names.txt' { "--- $($f.FullName.Substring($root.Length + 1))"; try { XmlNames $f.FullName } catch { "not read: $($_.Exception.Message)" } }
        }
    }
    'setup' {
        $root = Unpack
        $le = & (Join-Path $PSScriptRoot 'le.ps1') | Select-Object -Last 1
        # LR2body.exe first: its error, now readable
        $p = Start-Process -FilePath $le -ArgumentList @("`"$(Join-Path $root 'LR2body.exe')`"") -WorkingDirectory $root -PassThru
        Start-Sleep -Seconds 4
        Shot 'le-body.png'
        Log 'le-body.txt' { & $Probe windows; '--- the error'; & $Probe ui 'エラー' dump }
        Get-Process -Name LR2body -ErrorAction SilentlyContinue | Stop-Process -Force
        # then the setup dialog, tab by tab (the tabs' places in the dialog,
        # from its screenshot: the tab control has no other way in from
        # outside)
        $p = Start-Process -FilePath $le -ArgumentList @("`"$(Join-Path $root 'LR2.exe')`"") -WorkingDirectory $root -PassThru
        Start-Sleep -Seconds 4
        $tabs = [ordered]@{ main = 36; jukebox1 = 91; jukebox2 = 163; option = 224; select = 277; system = 333 }
        foreach ($t in $tabs.Keys) {
            & $Probe ui 'LR2 SETUP' mouse $tabs[$t] 74 | Out-Null
            Start-Sleep -Milliseconds 500
            Shot "setup-$t.png"
            Log 'setup-controls.txt' { "=== $t"; & $Probe ui 'LR2 SETUP' dump }
        }
        Get-Process -Name LR2 -ErrorAction SilentlyContinue | Stop-Process -Force
    }
    'player' {
        # a player and a song folder, through the setup dialog: what each
        # button opens is recorded (screenshot, controls)
        $root = Unpack
        $le = & (Join-Path $PSScriptRoot 'le.ps1') | Select-Object -Last 1
        $p = Start-Process -FilePath $le -ArgumentList @("`"$(Join-Path $root 'LR2.exe')`"") -WorkingDirectory $root -PassThru
        Start-Sleep -Seconds 4
        $n = 0
        function Popups([string] $tag) {
            Start-Sleep -Seconds 2
            Shot "player-$tag.png"
            $ids = @(Get-Process -Name LR2, LR2body -ErrorAction SilentlyContinue | ForEach-Object Id)
            $wins = & $Probe windows | Where-Object { $_ -match '^(0x[0-9A-Fa-f]+) pid (\d+) \[([^\]]*)\] "(.*)"' -and $ids -contains [int]$Matches[2] -and $Matches[4] -ne 'LR2 SETUP' }
            Log 'player.txt' { "=== $tag"; $wins }
            foreach ($w in $wins) {
                $null = $w -match '^(0x[0-9A-Fa-f]+)'
                Log 'player.txt' { "--- controls of $($Matches[1])"; & $Probe ui $Matches[1] dump }
            }
            return $wins
        }
        function Dismiss($wins) {
            foreach ($w in $wins) {
                if ($w -match '^(0x[0-9A-Fa-f]+) .*\[#32770\]') { & $Probe ui $Matches[1] key 0D | Out-Null; Start-Sleep -Milliseconds 500 }
            }
        }
        # a new player: e2etest / e2e
        & $Probe ui 'LR2 SETUP' click p/1003
        Start-Sleep -Seconds 1
        & $Probe ui 'NEW PLAYER' text 1002 e2etest
        & $Probe ui 'NEW PLAYER' text 1008 e2e
        & $Probe ui 'NEW PLAYER' click 1
        Dismiss (Popups 'new-player')
        Log 'player.txt' { '=== players'; & $Probe ui 'LR2 SETUP' items p/1001 }
        Log 'player.txt' { & $Probe ui 'LR2 SETUP' select p/1001 e2etest }
        Dismiss (Popups 'select-player')
        & $Probe ui 'LR2 SETUP' text p/1002 e2e
        & $Probe ui 'LR2 SETUP' check p/1006 1
        Log 'player.txt' { '=== main'; & $Probe ui 'LR2 SETUP' dump | Where-Object { $_ -notmatch 'hidden' } }
        $tabs = [ordered]@{ main = 36; jukebox1 = 91; jukebox2 = 163; option = 224; select = 277; system = 333 }
        # the charts, dropped onto JUKEBOX1's folder tree (the tab warns
        # first that no folder is registered)
        $songs = Join-Path $root 'songs'
        New-Item -ItemType Directory -Force (Join-Path $songs 'lr2-measure') | Out-Null
        Copy-Item (Join-Path $PSScriptRoot '..\..\lr2-measure\charts\*.bme') (Join-Path $songs 'lr2-measure')
        & $Probe ui 'LR2 SETUP' mouse $tabs['jukebox1'] 74 | Out-Null
        Dismiss (Popups 'tab-jukebox1')
        Log 'player.txt' { '=== drop'; & $Probe drop 'LR2 SETUP' 300 270 $songs }
        Dismiss (Popups 'drop')
        Shot 'player-dropped.png'
        foreach ($t in 'jukebox2', 'option', 'select', 'system', 'main') {
            & $Probe ui 'LR2 SETUP' mouse $tabs[$t] 74 | Out-Null
            Dismiss (Popups "tab-$t")
            Log 'player.txt' { "=== tab $t"; & $Probe ui 'LR2 SETUP' dump | Where-Object { $_ -notmatch 'hidden' -and $_ -match 'p/' } }
        }
        # LR2 itself
        & $Probe ui 'LR2 SETUP' click 1003
        foreach ($t in 5, 15, 30) {
            Start-Sleep -Seconds 5
            Shot "player-lr2-$t.png"
            Log 'player.txt' { "=== LR2 started, about $t s"; & $Probe windows | Where-Object { $_ -match 'LR2|Lunatic|#32770' } }
            $null = Popups "lr2-$t"
        }
        Get-Process -Name LR2body, LR2 -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
        Start-Sleep -Seconds 3
        Get-Process -Name LR2body, LR2 -ErrorAction SilentlyContinue | Stop-Process -Force
        Log 'player.txt' { '--- files'; Get-ChildItem (Join-Path $root 'LR2files') -Recurse -File | Where-Object { $_.LastWriteTime -gt (Get-Date).AddMinutes(-5) } | ForEach-Object { "$($_.FullName.Substring($root.Length + 1)) $($_.Length)" } }
        Get-ChildItem (Join-Path $root 'LR2files\Config') -Filter *.xml | ForEach-Object {
            $f = $_
            Log 'config-names.txt' { "--- $($f.Name)"; try { XmlNames $f.FullName } catch { "not read: $($_.Exception.Message)" } }
        }
    }
    default { throw "unknown stage $Stage" }
}
