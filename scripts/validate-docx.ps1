param([Parameter(Mandatory=$true)][string[]]$Document)
# Optional developer QA. Downloads Microsoft's validator into ignored qa-output;
# none of these .NET assemblies are dependencies of the desktop application.
$ErrorActionPreference = 'Stop'
$taskPackageRoot = Join-Path $PSScriptRoot '..\qa-output\openxml-sdk'
New-Item -ItemType Directory -Force -Path $taskPackageRoot | Out-Null
foreach ($taskPackage in @(@('documentformat.openxml','3.0.1'),@('documentformat.openxml.framework','3.0.1'),@('system.io.packaging','8.0.0'))) {
    $taskName=$taskPackage[0]
    $taskVersion=$taskPackage[1]
    $taskDirectory=Join-Path $taskPackageRoot $taskName
    if (-not (Test-Path -LiteralPath $taskDirectory)) {
        $taskArchive=Join-Path $taskPackageRoot ($taskName+'.zip')
        Invoke-WebRequest -Uri "https://api.nuget.org/v3-flatcontainer/$taskName/$taskVersion/$taskName.$taskVersion.nupkg" -OutFile $taskArchive -TimeoutSec 30
        Expand-Archive -LiteralPath $taskArchive -DestinationPath $taskDirectory
    }
}
foreach ($taskAssembly in @('system.io.packaging\lib\net8.0\System.IO.Packaging.dll','documentformat.openxml.framework\lib\net8.0\DocumentFormat.OpenXml.Framework.dll','documentformat.openxml\lib\net8.0\DocumentFormat.OpenXml.dll')) {
    [Reflection.Assembly]::LoadFrom((Join-Path $taskPackageRoot $taskAssembly)) | Out-Null
}
$taskValidator=[DocumentFormat.OpenXml.Validation.OpenXmlValidator]::new([DocumentFormat.OpenXml.FileFormatVersions]::Office2013)
foreach ($taskFile in $Document) {
    $taskPath=(Resolve-Path -LiteralPath $taskFile).Path
    $taskDocument=[DocumentFormat.OpenXml.Packaging.WordprocessingDocument]::Open($taskPath,$false)
    try {
        $taskErrors=@($taskValidator.Validate($taskDocument))
        if ($taskErrors.Count -gt 0) {
            $taskErrors | Select-Object -First 15 Description,@{n='XPath';e={$_.Path.XPath}},@{n='Part';e={$_.Part.Uri}} | Format-List
            throw "$taskFile : $($taskErrors.Count) Open XML schema errors"
        }
        Write-Output "$taskFile : 0 Open XML schema errors"
    } finally { $taskDocument.Dispose() }
}
