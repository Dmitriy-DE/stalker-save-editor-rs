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

# The portable Windows archive is built with the same MSVC target that the installer and the Windows tests use,
# so the shipped binaries are the ones CI runs.
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$distDir = Join-Path $repoRoot 'dist'
$stageDir = Join-Path $distDir 'windows-portable-stage'
$metadataDir = Join-Path $distDir '.package-metadata'
$target = 'x86_64-pc-windows-msvc'
$artifactName = 'SaveEditor-windows-x86_64.zip'
$sizeBudget = 31457280
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

$cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
Invoke-CheckedCommand -FilePath $cargo -ArgumentList @('build', '--locked', '--release', '--target', $target, '-p', 'sse-cli', '-p', 'sse-ui')

$releaseDir = Join-Path $repoRoot "target\$target\release"
$cliPath = Join-Path $releaseDir 'stalker-save.exe'
$shellPath = Join-Path $releaseDir 'sse-shell.exe'
foreach ($binary in @($cliPath, $shellPath)) {
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "Expected release binary was not produced: $binary"
    }
}
if ((Get-Item -LiteralPath $cliPath).Length -gt 3145728) {
    throw 'stalker-save.exe exceeds the 3 MiB CLI size budget'
}

New-Item -ItemType Directory -Force -Path $distDir, $metadataDir | Out-Null
if (Test-Path -LiteralPath $stageDir) {
    Remove-Item -LiteralPath $stageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
Copy-Item -LiteralPath $cliPath -Destination (Join-Path $stageDir 'stalker-save.exe')
Copy-Item -LiteralPath $shellPath -Destination (Join-Path $stageDir 'sse-shell.exe')
Copy-Item -LiteralPath (Join-Path $repoRoot 'packaging\icons\stalker-save-editor.svg') -Destination (Join-Path $stageDir 'stalker-save-editor.svg')
[System.IO.File]::WriteAllText(
    (Join-Path $stageDir 'BUILD_MANIFEST.json'),
    "{`n  `"target`": `"windows`",`n  `"architecture`": `"x86_64`",`n  `"kind`": `"portable`",`n  `"version`": `"$version`",`n  `"source_commit`": `"$commit`"`n}`n",
    [System.Text.UTF8Encoding]::new($false)
)

$archivePath = Join-Path $distDir $artifactName
if (Test-Path -LiteralPath $archivePath) {
    Remove-Item -LiteralPath $archivePath -Force
}
Compress-Archive -Path (Join-Path $stageDir '*') -DestinationPath $archivePath -CompressionLevel Optimal

# The archive must contain exactly the expected files, nothing else.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($archivePath)
try {
    $names = @($archive.Entries | ForEach-Object { $_.FullName } | Sort-Object)
} finally {
    $archive.Dispose()
}
$wanted = @('BUILD_MANIFEST.json', 'sse-shell.exe', 'stalker-save-editor.svg', 'stalker-save.exe') | Sort-Object
if (($names -join '|') -ne ($wanted -join '|')) {
    throw "Portable archive contents are wrong: $($names -join ', ')"
}

$size = (Get-Item -LiteralPath $archivePath).Length
if ($size -le 0 -or $size -gt $sizeBudget) {
    throw "Portable archive size $size is outside the 30 MiB release budget"
}
$sha256 = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
$utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)
$metadataLine = "$commit`t$version`twindows-x86_64`tx86_64`tportable`t$artifactName`t$size`t$sha256`n"
[System.IO.File]::WriteAllText((Join-Path $metadataDir 'windows-x86_64.tsv'), $metadataLine, $utf8WithoutBom)
[System.IO.File]::WriteAllText("$archivePath.sha256", "$sha256  $artifactName`n", $utf8WithoutBom)
Write-Output "Created $archivePath ($size bytes, sha256 $sha256)"
