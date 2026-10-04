# GPU matrix run (ZK-286, docs/GPU-MATRIX.md section 5): the latest Znimok release, its self-test
# (capture, recording, playback, export) in a library of its own, and what the GPU did, packed
# into one zip on the desktop. The installed Znimok, its settings and its library are not touched.
#
#   powershell -ExecutionPolicy Bypass -File gpu_matrix_run.ps1 [-Tag v0.0.16]
#
# Keep gpu_matrix_sample.png (the self-test's sample picture) next to this script.
#
# Takes 5-10 minutes; the self-test window opens and closes by itself - leave the mouse alone.

param([string]$Tag = "")

$ErrorActionPreference = "Stop"
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$work = Join-Path $env:TEMP "znimok-gpu-matrix-$stamp"
New-Item -ItemType Directory -Force -Path $work, "$work\st", "$work\lib" | Out-Null

# 1. The release
$api = if ($Tag) { "https://api.github.com/repos/V-Plum/znimok/releases/tags/$Tag" } else { "https://api.github.com/repos/V-Plum/znimok/releases/latest" }
$rel = Invoke-RestMethod -Uri $api -Headers @{ "User-Agent" = "znimok-gpu-matrix" }
$asset = $rel.assets | Where-Object { $_.name -like "*windows-x64.zip" } | Select-Object -First 1
if (-not $asset) { throw "no Windows zip in release $($rel.tag_name)" }
Write-Host "Znimok $($rel.tag_name): downloading $($asset.name)..."
$zip = Join-Path $work $asset.name
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
Expand-Archive -Path $zip -DestinationPath "$work\app" -Force
$exe = Get-ChildItem -Path "$work\app" -Recurse -Filter "znimok-app.exe" | Select-Object -First 1
if (-not $exe) { throw "znimok-app.exe not found in the zip" }

# 2. A picture to start from: the self-test's own sample next to this script (its checks are
#    tuned to it - a plain picture fails the guides' contrast and what follows); drawn if missing.
$sample = Join-Path $PSScriptRoot "gpu_matrix_sample.png"
if (-not (Test-Path $sample)) {
    Write-Host "gpu_matrix_sample.png is not next to the script: a plain picture instead (some checks may fail)"
    Add-Type -AssemblyName System.Drawing
    $bmp = New-Object System.Drawing.Bitmap 1600, 1000
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear([System.Drawing.Color]::FromArgb(243, 244, 248))
    $g.FillRectangle([System.Drawing.Brushes]::SteelBlue, 100, 100, 600, 300)
    $g.FillRectangle([System.Drawing.Brushes]::DimGray, 800, 500, 500, 60)
    $g.Dispose()
    $sample = Join-Path $work "sample.png"
    $bmp.Save($sample, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

# 3. The self-test, in its own library
$logDir = Join-Path $env:LOCALAPPDATA "Znimok\logs"
$before = @{}
if (Test-Path $logDir) {
    Get-ChildItem $logDir -Filter "znimok.log.*" | ForEach-Object { $before[$_.Name] = (Get-Content $_.FullName).Count }
}
Write-Host "Self-test running (5-10 min), please do not touch the mouse..."
$env:ZNIMOK_SELFTEST = "$work\st"
$env:ZNIMOK_LIBRARY = "$work\lib"
$p = Start-Process -FilePath $exe.FullName -ArgumentList "`"$sample`"" -PassThru `
    -RedirectStandardOutput "$work\st\console.txt" -RedirectStandardError "$work\st\stderr.txt"
if (-not $p.WaitForExit(900000)) { $p.Kill(); Write-Host "Self-test timed out after 15 min" }
Remove-Item Env:ZNIMOK_SELFTEST, Env:ZNIMOK_LIBRARY

# 4. What the GPU did: the log lines written during the run
$lines = @()
if (Test-Path $logDir) {
    Get-ChildItem $logDir -Filter "znimok.log.*" | ForEach-Object {
        $all = @(Get-Content -Encoding UTF8 $_.FullName)
        $from = if ($before.ContainsKey($_.Name)) { $before[$_.Name] } else { 0 }
        if ($all.Count -gt $from) { $lines += $all[$from..($all.Count - 1)] }
    }
}
$lines | Set-Content -Encoding UTF8 "$work\st\log-during-run.txt"

$gpu = Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion, DriverDate, AdapterRAM, VideoProcessor
$os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber
$cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
$console = if (Test-Path "$work\st\console.txt") { Get-Content -Encoding UTF8 "$work\st\console.txt" } else { @() }
$summary = @(
    "Znimok $($rel.tag_name) on $env:COMPUTERNAME, $stamp",
    "OS: $($os.Caption) $($os.Version) build $($os.BuildNumber)",
    "CPU: $cpu",
    "GPUs:"
) + ($gpu | ForEach-Object { "  $($_.Name) | driver $($_.DriverVersion) ($($_.DriverDate))" }) + @(
    "",
    "Self-test: $(($console | Select-Object -Last 1))",
    "Failures:"
) + ($console | Where-Object { $_ -like "FAIL*" }) + @(
    "",
    "GPU lines from the log:"
) + ($lines | Where-Object { $_ -match "wgpu: |recording: |player|export|encoder|WARN|ERROR" })
$summary | Set-Content -Encoding UTF8 "$work\st\summary.txt"

# 5. One zip on the desktop (no screenshots of the person's screen: only the self-test's own window)
$out = Join-Path ([Environment]::GetFolderPath("Desktop")) "znimok-gpu-matrix-$env:COMPUTERNAME-$stamp.zip"
Compress-Archive -Path "$work\st\*" -DestinationPath $out -Force
Write-Host ""
$summary | Select-Object -First 12 | ForEach-Object { Write-Host $_ }
Write-Host ""
Write-Host "Done: $out"
Write-Host "Send this zip back. The temporary folder $work can be deleted."
