#!/usr/bin/env pwsh
# One-time per-clone setup: pin every PR / push operation to the bbfox0703 fork
# so they never target the `potter420/crimson-rs` upstream.
#
# This is a personal-playground fork (see CLAUDE.md). `gh` defaults pr/issue
# commands to the *parent* repo for forks, so `gh pr create` keeps trying to
# open the PR against potter420 and failing ("No commits between main and dev /
# Head ref must be a branch"). The upstream is a ~1.03-era parent with nothing
# left to contribute to, so nothing from this clone should ever be pushed there.
# These guards live in the local `.git/config`, `.git/hooks/` and gh's
# resolved-default, so a fresh clone resets them — re-run this script:
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

# 4. A pre-push hook refuses any push whose destination is potter420/crimson-rs,
#    however it is spelled — so `git push https://github.com/potter420/...`, a
#    re-added remote or a reset push URL cannot get past step 3 either. Only
#    `--no-verify` skips it. Written with LF endings: Git for Windows runs hooks
#    through sh, which chokes on a CRLF shebang.
$hookMarker = 'crimson-rs fork guard'
# Absolute: the .NET write below resolves relative paths against the process
# directory, which need not be PowerShell's current location.
$hookPath = Join-Path (Resolve-Path (git rev-parse --git-path hooks)).Path 'pre-push'
$hook = @"
#!/bin/sh
# $hookMarker — installed by scripts/setup-fork-guards.ps1. Refuses to push
# to the potter420/crimson-rs upstream; push to origin (bbfox0703) instead.
url=`$(printf '%s' "`$2" | tr 'A-Z' 'a-z')
case "`$url" in
    *potter420/crimson-rs*)
        echo "pre-push: refusing to push to the potter420 upstream (`$2)." >&2
        echo "pre-push: this fork pushes to origin (bbfox0703/crimson-rs) only." >&2
        exit 1 ;;
esac
exit 0
"@
if ((Test-Path $hookPath) -and -not (Select-String -Path $hookPath -Quiet -SimpleMatch $hookMarker)) {
    Write-Warning "$hookPath exists and is not ours; left it alone (fork-guard hook NOT installed)."
} else {
    [IO.File]::WriteAllText($hookPath, $hook.Replace("`r`n", "`n") + "`n", [Text.UTF8Encoding]::new($false))
}

Write-Output 'Fork guards set:'
Write-Output "  gh default repo : $(gh repo set-default --view 2>&1)"
Write-Output "  push default    : $(git config --get remote.pushDefault)"
try { Write-Output "  upstream push   : $(git remote get-url --push upstream)" } catch {}
Write-Output "  pre-push hook   : $hookPath"
Write-Output ''
Write-Output 'PRs: gh pr create --base main --head dev   (now resolves to bbfox0703/crimson-rs)'
