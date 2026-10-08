$ErrorActionPreference = 'Stop'

function Invoke-CheckedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ArgumentList
    )

    & $FilePath @ArgumentList
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath exited with code $LASTEXITCODE"
    }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$distDir = Join-Path $repoRoot 'dist'
$stageDir = Join-Path $distDir 'windows-installer-stage'
$metadataDir = Join-Path $distDir '.package-metadata'
$target = 'x86_64-pc-windows-msvc'
$artifactName = 'SaveEditor-windows-x86_64-setup.exe'
$workspaceToml = [System.IO.File]::ReadAllText((Join-Path $repoRoot 'Cargo.toml'))
$workspaceSection = [regex]::Match($workspaceToml, '(?ms)^\[workspace\.package\]\s*$(?<body>.*?)(?=^\[|\z)')
$versionMatch = [regex]::Match($workspaceSection.Groups['body'].Value, '(?m)^version\s*=\s*"([^"]+)"\s*$')
if (-not $versionMatch.Success) {
    throw 'Could not read [workspace.package].version from Cargo.toml'
}
$version = $versionMatch.Groups[1].Value
$commit = (& git -C $repoRoot rev-parse --verify HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $commit -notmatch '^[0-9a-f]{40,64}$') {
    throw 'Could not read the full source commit SHA'
}

$rustup = (Get-Command rustup.exe -ErrorAction Stop).Source
$cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
Invoke-CheckedCommand -FilePath $rustup -ArgumentList @('target', 'add', $target)
Invoke-CheckedCommand -FilePath $cargo -ArgumentList @('build', '--locked', '--release', '--target', $target, '-p', 'sse-cli', '-p', 'sse-ui')

$releaseDir = Join-Path $repoRoot "target\$target\release"
$cliPath = Join-Path $releaseDir 'stalker-save.exe'
$shellPath = Join-Path $releaseDir 'sse-shell.exe'
foreach ($binary in @($cliPath, $shellPath)) {
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "Expected release binary was not produced: $binary"
    }
}

New-Item -ItemType Directory -Force -Path $distDir, $metadataDir | Out-Null
if (Test-Path -LiteralPath $stageDir) {
    Remove-Item -LiteralPath $stageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
Copy-Item -LiteralPath $cliPath -Destination (Join-Path $stageDir 'stalker-save.exe')
Copy-Item -LiteralPath $shellPath -Destination (Join-Path $stageDir 'sse-shell.exe')
[System.IO.File]::WriteAllText(
    (Join-Path $stageDir 'BUILD_MANIFEST.json'),
    "{`n  `"target`": `"windows`",`n  `"architecture`": `"x86_64`",`n  `"kind`": `"installer`",`n  `"version`": `"$version`",`n  `"source_commit`": `"$commit`"`n}`n",
    [System.Text.UTF8Encoding]::new($false)
)
[System.IO.File]::WriteAllText(
    (Join-Path $stageDir 'INSTALLER_MARKER'),
    'installer',
    [System.Text.UTF8Encoding]::new($false)
)

$innoCandidates = @()
$innoCommand = Get-Command ISCC.exe -ErrorAction SilentlyContinue
if ($null -ne $innoCommand) {
    $innoCandidates += $innoCommand.Source
}
foreach ($programFilesDir in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
    if (-not [string]::IsNullOrWhiteSpace($programFilesDir)) {
        $innoCandidates += (Join-Path $programFilesDir 'Inno Setup 6\ISCC.exe')
    }
}
$iscc = $innoCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if ($null -eq $iscc) {
    throw 'Inno Setup 6 ISCC.exe was not found'
}

$installerPath = Join-Path $distDir $artifactName
if (Test-Path -LiteralPath $installerPath) {
    Remove-Item -LiteralPath $installerPath -Force
}
$issPath = Join-Path $repoRoot 'packaging\installer.iss'
Invoke-CheckedCommand -FilePath $iscc -ArgumentList @(
    "/DAppVersion=$version",
    "/DSourceDir=$stageDir",
    "/DOutputDir=$distDir",
    $issPath
)
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
    throw "Inno Setup did not produce the expected installer: $installerPath"
}

$size = (Get-Item -LiteralPath $installerPath).Length
if ($size -le 0 -or $size -gt 30MB) {
    throw "Installer size $size is outside the 30 MiB release budget"
}
$sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$metadataLine = "$commit`t$version`twindows-installer-x86_64`tx86_64`tinstaller`t$artifactName`t$size`t$sha256`n"
$utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText((Join-Path $metadataDir 'windows-installer-x86_64.tsv'), $metadataLine, $utf8WithoutBom)
[System.IO.File]::WriteAllText("$installerPath.sha256", "$sha256  $artifactName`n", $utf8WithoutBom)
Write-Output "Created $installerPath ($size bytes, sha256 $sha256)"
