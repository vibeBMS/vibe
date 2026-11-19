# Sets up Locale Emulator (open source, LGPL) in $env:RUNNER_TEMP\le and
# prints the path of LEProc.exe. `LEProc.exe PATH ARGS` runs PATH with LE's
# default ja-JP profile: code page 932, as on a Japanese system.
$ErrorActionPreference = 'Stop'
$le = Join-Path $env:RUNNER_TEMP 'le'
$leproc = Get-ChildItem $le -Recurse -Filter LEProc.exe -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $leproc) {
    New-Item -ItemType Directory -Force $le | Out-Null
    & gh release download --repo xupefei/Locale-Emulator --pattern '*.zip' --dir $le --clobber | Out-Host
    if ($LASTEXITCODE) { throw 'Locale Emulator: download failed' }
    Get-ChildItem $le -Filter *.zip | ForEach-Object { Write-Host "Locale Emulator $($_.Name)"; Expand-Archive $_.FullName -DestinationPath $le -Force }
    $leproc = Get-ChildItem $le -Recurse -Filter LEProc.exe | Select-Object -First 1
}
# LEProc needs LECommonLibrary.dll, which only LEInstaller (a window with an
# Install button) writes out, from its resources: written here from the same
# resource, without the window
$dll = Join-Path $leproc.DirectoryName 'LECommonLibrary.dll'
if (-not (Test-Path $dll)) {
    $cmd = "`$a = [Reflection.Assembly]::LoadFile('$(Join-Path $leproc.DirectoryName 'LEInstaller.exe')'); " +
        "`$n = `$a.GetManifestResourceNames() | Where-Object { `$_ -like '*.Resources.resources' } | Select-Object -First 1; " +
        "`$rm = New-Object Resources.ResourceManager(`$n -replace '\.resources$', ''), `$a; " +
        "[IO.File]::WriteAllBytes('$dll', [byte[]] `$rm.GetObject('LECommonLibrary'))"
    & powershell.exe -NoProfile -Command $cmd | Out-Host
    Write-Host "LECommonLibrary.dll from LEInstaller: $((Get-Item $dll -ErrorAction SilentlyContinue).Length) bytes"
}
$leproc.FullName
