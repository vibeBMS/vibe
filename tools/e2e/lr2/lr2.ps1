# Runs LR2 as a black box on the runner: starts it, reads its screen
# (gdigrab) and the files it writes. LR2's own files stay in
# $env:RUNNER_TEMP\lr2; $Out gets only this script's logs and screenshots.
#   lr2.ps1 -Stage explore -Zip LR2.zip -Probe lr2probe.exe -Out DIR
param(
    [Parameter(Mandatory)] [string] $Stage,
    [Parameter(Mandatory)] [string] $Zip,
    [Parameter(Mandatory)] [string] $Probe,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
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
    default { throw "unknown stage $Stage" }
}
