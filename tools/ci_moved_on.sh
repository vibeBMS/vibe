#!/usr/bin/env bash
# tools/ci_moved_on.sh <commit>: "yes" when the branch (GITHUB_REF_NAME, in
# GH_REPO) has moved on from <commit> to one that CI builds, "no" when
# <commit> is its head or every newer commit skips CI ([skip ci], [ci skip],
# [no ci], [skip actions] or [actions skip] in its message: such a push starts
# no run, so <commit>'s run is the latest build); fails when GitHub's API
# does. The nightly job publishes only on "no".
set -euo pipefail
sha="$1"
head="$(gh api "repos/$GH_REPO/git/ref/heads/$GITHUB_REF_NAME" --jq .object.sha)"
if [ "$head" = "$sha" ]; then echo no; exit 0; fi
gh api "repos/$GH_REPO/compare/$sha...$head" --jq '
  if .status == "ahead" and .total_commits == (.commits | length) and all(.commits[];
    .commit.message | test("\\[(skip ci|ci skip|no ci|skip actions|actions skip)\\]"; "i"))
  then "no" else "yes" end'
