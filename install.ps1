# Installs claudeconnect on Windows: downloads the latest release, puts it on PATH and
# registers it with Claude Code.
#   irm https://raw.githubusercontent.com/nx01-600/claudeconnect/main/install.ps1 | iex
$ErrorActionPreference = 'Stop'
$repo = 'nx01-600/claudeconnect'

if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
    Write-Warning 'Claude Code (claude) is not on PATH. Install it first: https://code.claude.com'
    return
}

$dir = Join-Path $env:USERPROFILE '.claudeconnect\bin'
$exe = Join-Path $dir 'claudeconnect.exe'
New-Item -ItemType Directory -Force $dir | Out-Null

$url = "https://github.com/$repo/releases/latest/download/claudeconnect-windows-x86_64.exe"
Write-Host "Downloading $url"
Invoke-WebRequest $url -OutFile "$exe.download" -UseBasicParsing

# Open Claude Code sessions keep the old exe running: it can't be overwritten, but it can be renamed.
if (Test-Path $exe) { Move-Item -Force $exe "$exe.$(Get-Date -Format yyyyMMddHHmmss).old" }
Move-Item -Force "$exe.download" $exe
Get-ChildItem $dir -Filter '*.old' | Remove-Item -ErrorAction SilentlyContinue

& $exe install
