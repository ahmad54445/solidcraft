# [CUSTOM] Pull the latest SolidCraft into the fork and check the adapter layer.
# Usage (from the repo root):  powershell -ExecutionPolicy Bypass -File custom\update-upstream.ps1 [-Ref v0.40.0]
param([string]$Ref = "upstream/main")

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

if (git status --porcelain) { Write-Host "Uncommitted changes: commit or stash them first." -ForegroundColor Red; exit 1 }

git fetch upstream --tags
git switch main
git merge --ff-only upstream/main
git switch factory

Write-Host "`n== Merging $Ref into 'factory' ==" -ForegroundColor Cyan
git merge --no-edit $Ref
if ($LASTEXITCODE -ne 0) {
    Write-Host "`nMerge conflicts. They can only be in the [CUSTOM] lines listed in custom/README.md." -ForegroundColor Yellow
    git diff --name-only --diff-filter=U
    exit 1
}

Write-Host "`n== Checking the adapter layer ==" -ForegroundColor Cyan
cargo test -p custom-bridge -p custom-features --features custom-bridge/ui
if ($LASTEXITCODE -ne 0) {
    Write-Host "`nSomething upstream changed. Fix it in custom/bridge (usually src/names.rs), not in custom/features." -ForegroundColor Yellow
    exit 1
}

Write-Host "`n== Files of upstream the fork touches ==" -ForegroundColor Cyan
git diff --stat upstream/main...factory -- . ":(exclude)custom"
Write-Host "`nUpdate OK." -ForegroundColor Green
