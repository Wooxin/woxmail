param(
  [string]$RepoName = "woxmail",
  [string]$Proxy = "http://localhost:10808",
  [switch]$Public
)

$ErrorActionPreference = "Stop"

if ($Proxy) {
  $env:HTTP_PROXY = $Proxy
  $env:HTTPS_PROXY = $Proxy
  $env:ALL_PROXY = $Proxy
}

$gh = "gh"
if (-not (Get-Command $gh -ErrorAction SilentlyContinue)) {
  $installedGh = Join-Path $env:ProgramFiles "GitHub CLI\gh.exe"
  if (Test-Path $installedGh) {
    $gh = $installedGh
  } else {
    throw "GitHub CLI is not installed. Install it with: winget install --id GitHub.cli"
  }
}

& $gh auth status 2>$null
if ($LASTEXITCODE -ne 0) {
  & $gh auth login --hostname github.com --git-protocol https --web --scopes repo,workflow
}

$visibility = if ($Public) { "--public" } else { "--private" }
& $gh repo create $RepoName $visibility --source . --remote origin --push

Write-Host "Published repository: $RepoName"
