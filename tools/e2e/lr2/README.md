# e2e: LR2 on a hosted Windows runner

Phase 2 of the e2e plan: Lunatic Rave 2 (100201) run as a black box on `windows-latest`, to play the
`tools/lr2-measure` charts unattended and compare its result counts with vibeBMS's rows
(`crates/vibebms-core/tests/lr2_measure.rs`). The workflow is `.github/workflows/e2e-lr2.yml`.

LR2 is only run, and only its screen and the files it writes are read. LR2 comes from its official site, once,
into the Actions cache keyed by its SHA-256; no LR2 file is committed or uploaded as an artifact (the artifact
`e2e-lr2` holds this workflow's own logs, screenshots and tables).

Status: findings below.
