# CI action runtime maintenance

[Issue #10](https://github.com/frankischilling/26chan/issues/10) tracks warnings from Node.js 20 runtimes in the pinned checkout and setup-node actions. Both workflows now use official releases whose action manifests specify `node24`:

| Action | Release | Verified commit |
|---|---|---|
| actions/checkout | [7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1) | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| actions/setup-node | [7.0.0](https://github.com/actions/setup-node/releases/tag/v7.0.0) | `820762786026740c76f36085b0efc47a31fe5020` |

On September 8, 2026, `gh api repos/actions/checkout/git/ref/tags/v7.0.1 --jq '.object'` and `gh api repos/actions/setup-node/git/ref/tags/v7.0.0 --jq '.object'` returned those commit objects. The corresponding release notes, manifests and READMEs were checked at the pinned revisions. The minimum supported runner is 2.327.1. See [checkout runtime requirements](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/README.md) and [setup-node runtime/cache behavior](https://github.com/actions/setup-node/blob/820762786026740c76f36085b0efc47a31fe5020/README.md).

The diff preserves `contents: read`, `persist-credentials: false`, all triggers, hosted runner labels, explicit tool versions and verification commands. It adds `package-manager-cache: false` to retain the previous absence of a package cache despite newer automatic cache detection. It adds no repository secrets, privileged trigger or unsafe checkout override. No application code, dependency lockfile or screenshot baseline changed.

Local verification uses actionlint 1.7.12, built from the official Go module into ignored `.local/tools`; no global tool configuration was changed. `.\.local\tools\actionlint.exe .github/workflows/ci.yml .github/workflows/advisories.yml` and `git diff --check` passed. Application tests are exercised by the hosted workflows; a static lint pass alone does not verify either hosted platform.

The local Python environment lacked PyYAML, so a preliminary import check failed with `ModuleNotFoundError`. Workflow validation used actionlint instead; no Python package or runtime application dependency was added. Installation command: `$env:GOBIN = Join-Path (Get-Location) '.local\tools'; go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.12`.

Hosted results will be recorded on the draft PR after execution. This maintenance branch starts from the merged API checkpoint. The separate staff inactivity change in PR #12 has its own migration and test evidence. Neither PR establishes deployed media containment, physical authenticator protection or full compatibility with an approved original reference. No production deployment or merge is included.
