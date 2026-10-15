param([string]$Executable = "$PSScriptRoot\..\target\release\md-to-word.exe")
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path -LiteralPath "$PSScriptRoot\..").Path
$taskOutput = Join-Path $taskRoot 'qa-output'
New-Item -ItemType Directory -Force -Path $taskOutput | Out-Null
$taskExecutable = (Resolve-Path -LiteralPath $Executable).Path
$taskResults = @()
foreach ($taskCase in @(@{Name='small'; Count=100}, @{Name='large'; Count=16000})) {
    $taskInput = Join-Path $taskOutput ($taskCase.Name + '.md')
    $taskDestination = Join-Path $taskOutput ($taskCase.Name + '.docx')
    $taskBlock = @'
## 数学练习

已知 $a=-2$，计算 $a^2+3a$。请列出步骤，说明理由。

$$
\frac{1}{2}+\frac{1}{3}=\frac{5}{6}
$$

- 保留计算步骤
- 检查最终结果

'@
    [IO.File]::WriteAllText($taskInput, ($taskBlock + "`n") * $taskCase.Count, [Text.UTF8Encoding]::new($false))
    $taskInfo = [Diagnostics.ProcessStartInfo]::new($taskExecutable)
    $taskInfo.UseShellExecute = $false
    $taskInfo.CreateNoWindow = $true
    $taskInfo.RedirectStandardOutput = $true
    $taskInfo.RedirectStandardError = $true
    foreach ($taskArgument in @('--convert', $taskInput, $taskDestination, '--force')) { $taskInfo.ArgumentList.Add($taskArgument) }
    $taskClock = [Diagnostics.Stopwatch]::StartNew()
    $taskProcess = [Diagnostics.Process]::Start($taskInfo)
    $taskPeakWorking = 0L
    $taskPeakPrivate = 0L
    while (-not $taskProcess.HasExited) {
        try {
            $taskProcess.Refresh()
            $taskPeakWorking = [Math]::Max($taskPeakWorking, $taskProcess.PeakWorkingSet64)
            $taskPeakPrivate = [Math]::Max($taskPeakPrivate, $taskProcess.PrivateMemorySize64)
        } catch { }
        Start-Sleep -Milliseconds 20
    }
    $taskClock.Stop()
    $taskLog = $taskProcess.StandardOutput.ReadToEnd()
    $taskError = $taskProcess.StandardError.ReadToEnd()
    if ($taskProcess.ExitCode -ne 0) { throw "Conversion failed: $taskError" }
    $taskResults += [PSCustomObject]@{
        case = $taskCase.Name
        markdownBytes = (Get-Item -LiteralPath $taskInput).Length
        docxBytes = (Get-Item -LiteralPath $taskDestination).Length
        seconds = [Math]::Round($taskClock.Elapsed.TotalSeconds, 2)
        peakWorkingSetMiB = [Math]::Round($taskPeakWorking / 1MB, 2)
        sampledPeakPrivateMiB = [Math]::Round($taskPeakPrivate / 1MB, 2)
        summary = $taskLog.Trim()
    }
    $taskProcess.Dispose()
}
$taskResults | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOutput 'performance.json') -Encoding utf8
$taskResults | ConvertTo-Json
