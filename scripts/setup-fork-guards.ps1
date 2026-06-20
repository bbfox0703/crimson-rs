#!/usr/bin/env pwsh
# One-time per-clone setup: pin every PR / push operation to the bbfox0703 fork
# so they never target the `potter420/crimson-rs` upstream.
#
# This is a personal-playground fork (see CLAUDE.md). `gh` defaults pr/issue
# commands to the *parent* repo for forks, so `gh pr create` keeps trying to
# open the PR against potter420 and failing ("No commits between main and dev /
# Head ref must be a branch"). These guards live in the local `.git/config` (and
# gh's resolved-default), so a fresh clone resets them — re-run this script:
#
#     pwsh scripts/setup-fork-guards.ps1
#
$ErrorActionPreference = 'Stop'

# 1. gh pr / list / checks / view default to the fork, not the upstream parent.
gh repo set-default bbfox0703/crimson-rs

# 2. A bare `git push` always goes to the fork.
git config remote.pushDefault origin

# 3. `git push upstream ...` errors out instead of pushing to potter420. Fetch
#    is left intact so upstream changes can still be pulled for reference.
if (git remote | Select-String -Quiet '^upstream$') {
    git remote set-url --push upstream DISABLED_no_push_to_potter420
}

Write-Output 'Fork guards set:'
Write-Output "  gh default repo : $(gh repo set-default --view 2>&1)"
Write-Output "  push default    : $(git config --get remote.pushDefault)"
try { Write-Output "  upstream push   : $(git remote get-url --push upstream)" } catch {}
Write-Output ''
Write-Output 'PRs: gh pr create --base main --head dev   (now resolves to bbfox0703/crimson-rs)'
