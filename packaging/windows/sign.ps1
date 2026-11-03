# Authenticode-signs files with Azure Artifact Signing (formerly
# "Trusted Signing") via signtool.exe and Microsoft's Artifact Signing
# dlib. Called by ci.yml's Windows job: on badterm.exe before it is zipped
# and put in the installer, then on the finished installer. Taken from
# ZephyrFTP's tools/windows-sign.ps1, which proved it first.
#
#   powershell packaging\windows\sign.ps1 target\release\badterm.exe
#
# Environment (Forgejo secrets on the arelas account, shared with ZephyrFTP):
#   AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET
#       an Entra app registration (service principal) holding the
#       "Artifact Signing Certificate Profile Signer" role on the
#       certificate profile. Read by the dlib's DefaultAzureCredential
#       (EnvironmentCredential); every other credential type is excluded
#       below so a runner-local az/VS login can never be picked up.
#   AZURE_SIGNING_ENDPOINT   e.g. https://eus.codesigning.azure.net; it
#       must match the signing account's region (a mismatch is a 403).
#   AZURE_SIGNING_ACCOUNT    the Artifact Signing account name.
#   AZURE_SIGNING_PROFILE    the certificate profile name.
#
# Tools are fetched from NuGet into a runner-persistent cache rather than
# installed machine-wide, so nothing needs admin on the runner:
#   Microsoft.Windows.SDK.BuildTools  (signtool.exe; the dlib needs a newer
#                                      one than older VS-installed SDKs ship)
#   Microsoft.ArtifactSigning.Client  (Azure.CodeSigning.Dlib.dll)
# The dlib also needs the .NET 8 runtime: the machine-wide one if present,
# otherwise one installed per-user into the same cache (see below).
param([Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)][string[]]$Files)

$ErrorActionPreference = 'Stop'

foreach ($v in 'AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET',
               'AZURE_SIGNING_ENDPOINT', 'AZURE_SIGNING_ACCOUNT', 'AZURE_SIGNING_PROFILE') {
    if (-not [Environment]::GetEnvironmentVariable($v)) {
        Write-Host "::error::$v is not set"
        exit 1
    }
}

$cache = Join-Path $env:LOCALAPPDATA 'badterm-signing-tools'
New-Item -ItemType Directory -Force -Path $cache | Out-Null

# The dlib is .NET 8 code loaded into native signtool.exe, which finds the
# runtime through DOTNET_ROOT or else the machine-wide install folder.
# Checked on disk rather than via `dotnet --list-runtimes`: ZephyrFTP's
# v0.8.39 run died on exactly that — `dotnet` wasn't on the runner service's PATH, and
# a runtime installed after the service started wouldn't be either. If no
# machine-wide 8.x exists, install one per-user into the tools cache with
# Microsoft's dotnet-install.ps1 (no admin needed) and point DOTNET_ROOT at
# it for signtool, which inherits this process's environment.
function Test-Net8([string]$root) {
    $shared = Join-Path $root 'shared\Microsoft.NETCore.App'
    return [bool](Get-ChildItem -Path $shared -Directory -Filter '8.*' -ErrorAction SilentlyContinue)
}
$machineDotnet = Join-Path $env:ProgramFiles 'dotnet'
$localDotnet = Join-Path $cache 'dotnet'
if (Test-Net8 $machineDotnet) {
    Write-Host ".NET 8 runtime: $machineDotnet (machine-wide)"
} else {
    if (-not (Test-Net8 $localDotnet)) {
        Write-Host ".NET 8 runtime not installed machine-wide - installing per-user into $localDotnet"
        $installer = Join-Path $cache 'dotnet-install.ps1'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://dot.net/v1/dotnet-install.ps1' -OutFile $installer
        & $installer -Channel 8.0 -Runtime dotnet -Architecture x64 -InstallDir $localDotnet -NoPath
        if (-not (Test-Net8 $localDotnet)) { Write-Host "::error::per-user .NET 8 runtime install failed"; exit 1 }
    }
    $env:DOTNET_ROOT = $localDotnet
    $env:DOTNET_ROOT_X64 = $localDotnet
    Write-Host ".NET 8 runtime: $localDotnet (per-user, via DOTNET_ROOT)"
}
$nuget = Join-Path $cache 'nuget.exe'
if (-not (Test-Path $nuget)) {
    Invoke-WebRequest -UseBasicParsing -Uri 'https://dist.nuget.org/win-x86-commandline/latest/nuget.exe' -OutFile $nuget
}
foreach ($pkg in 'Microsoft.Windows.SDK.BuildTools', 'Microsoft.ArtifactSigning.Client') {
    if (-not (Test-Path (Join-Path $cache $pkg))) {
        & $nuget install $pkg -ExcludeVersion -NonInteractive -OutputDirectory $cache
        if ($LASTEXITCODE -ne 0) { Write-Host "::error::nuget install $pkg failed"; exit 1 }
    }
}

# Located rather than hardcoded: both packages nest their binaries under
# version-numbered folders that change between releases.
$signtool = Get-ChildItem -Path (Join-Path $cache 'Microsoft.Windows.SDK.BuildTools') -Recurse -Filter signtool.exe |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
$dlib = Get-ChildItem -Path (Join-Path $cache 'Microsoft.ArtifactSigning.Client') -Recurse -Filter Azure.CodeSigning.Dlib.dll |
    Where-Object { $_.Directory.Name -eq 'x64' } | Select-Object -First 1
if (-not $signtool) { Write-Host "::error::x64 signtool.exe not found under $cache"; exit 1 }
if (-not $dlib)     { Write-Host "::error::x64 Azure.CodeSigning.Dlib.dll not found under $cache"; exit 1 }
Write-Host "signtool: $($signtool.FullName)"
Write-Host "dlib:     $($dlib.FullName)"

$metadata = Join-Path ([IO.Path]::GetTempPath()) "badterm-signing-$PID.json"
@{
    Endpoint               = $env:AZURE_SIGNING_ENDPOINT
    CodeSigningAccountName = $env:AZURE_SIGNING_ACCOUNT
    CertificateProfileName = $env:AZURE_SIGNING_PROFILE
    CorrelationId          = "badterm-$env:GITHUB_REF_NAME-$env:GITHUB_RUN_NUMBER"
    ExcludeCredentials     = @(
        'ManagedIdentityCredential', 'WorkloadIdentityCredential', 'SharedTokenCacheCredential',
        'VisualStudioCredential', 'VisualStudioCodeCredential', 'AzureCliCredential',
        'AzurePowerShellCredential', 'AzureDeveloperCliCredential', 'InteractiveBrowserCredential'
    )
} | ConvertTo-Json | ForEach-Object {
    # Without a byte-order mark: Windows PowerShell 5.1 (what BadTerm's CI
    # runs) adds one for -Encoding UTF8, and the dlib reads plain JSON.
    [IO.File]::WriteAllText($metadata, $_, (New-Object Text.UTF8Encoding $false))
}

try {
    foreach ($f in $Files) {
        if (-not (Test-Path $f)) { Write-Host "::error::$f does not exist"; exit 1 }
        Write-Host "Signing $f"
        # Artifact Signing certificates are only valid for three days, so
        # the RFC 3161 timestamp is what keeps the signature valid after.
        & $signtool.FullName sign /v /fd SHA256 /tr 'http://timestamp.acs.microsoft.com' /td SHA256 `
            /dlib $dlib.FullName /dmdf $metadata $f
        if ($LASTEXITCODE -ne 0) { Write-Host "::error::signtool sign failed for $f"; exit 1 }
        & $signtool.FullName verify /pa /v $f
        if ($LASTEXITCODE -ne 0) { Write-Host "::error::signtool verify failed for $f"; exit 1 }
    }
} finally {
    Remove-Item -Force $metadata -ErrorAction SilentlyContinue
}
