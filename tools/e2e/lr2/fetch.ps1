# Downloads LR2 from its official site into $Out (a zip; never into the
# repository's tree and never uploaded) and checks its SHA-256.
#   fetch.ps1 -Url URL -Out FILE [-Sha256 HEX]
# Writes sha256=... to $GITHUB_OUTPUT. Fails, saying why, if the site does
# not answer: no mirror is tried (the owner's rule).
param([Parameter(Mandatory)] [string] $Url, [Parameter(Mandatory)] [string] $Out, [string] $Sha256 = '')
$ErrorActionPreference = 'Stop'
$uri = [Uri] $Url
Write-Host "LR2 from $Url"
try {
    $ips = Resolve-DnsName -Name $uri.Host -ErrorAction Stop | Where-Object { $_.IPAddress } | ForEach-Object { $_.IPAddress }
    Write-Host "DNS $($uri.Host): $($ips -join ', ')"
} catch {
    Write-Host "::error::DNS $($uri.Host): $($_.Exception.Message)"
    Write-Host "LR2's official download does not resolve from the runner; no mirror is tried."
    exit 1
}
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
try {
    Invoke-WebRequest -Uri $Url -OutFile $Out -TimeoutSec 120 -MaximumRedirection 5 -UseBasicParsing
} catch {
    Write-Host "::error::download: $($_.Exception.Message)"
    exit 1
}
$bytes = [IO.File]::ReadAllBytes($Out)
if ($bytes.Length -lt 4 -or $bytes[0] -ne 0x50 -or $bytes[1] -ne 0x4b) {
    # a page (a click-through, an error) instead of the zip
    Write-Host "::error::the download is not a zip ($($bytes.Length) bytes): the site wants a page visit, or the file is gone"
    Remove-Item $Out
    exit 1
}
$hash = (Get-FileHash -Algorithm SHA256 $Out).Hash.ToLower()
Write-Host "SHA-256 $hash, $($bytes.Length) bytes"
if ($Sha256 -and $hash -ne $Sha256.ToLower()) {
    Write-Host "::error::SHA-256 is $hash, the workflow pins $Sha256"
    Remove-Item $Out
    exit 1
}
if ($env:GITHUB_OUTPUT) { "sha256=$hash" | Out-File -Append -Encoding utf8 $env:GITHUB_OUTPUT }
