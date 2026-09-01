# Engineer Handoff: Python → Rust Migration of integrations-core

Companion to [`MIGRATION_PLAN.md`](./MIGRATION_PLAN.md) (the full phased plan). This document
is the onboarding/handoff: what this repo is, what has already been done and verified, how to
reproduce it, and every challenge you should expect.

## 1. Context: what this repo is and what the goal is

The Datadog Agent is a daemon customers run on their hosts to collect telemetry. This repo
holds ~265 **integrations** — Python plugins, one directory each (`postgres/`, `mysql/`,
`nginx/`, …) — that each know how to query one technology and turn the answers into Datadog
metrics (e.g. `postgres.connections`). They share:

- `datadog_checks_base/` — the framework every check builds on (`AgentCheck`, metric
  submission, config handling, HTTP wrapper, DB query executor, DBM async jobs).
- `datadog_checks_dev/` + `ddev/` — the dev/test CLI and fixtures.
- Per-integration docker compose environments for integration/E2E tests.
- Language-neutral contracts: `metadata.csv` (metric catalog), `manifest.json`,
  `assets/configuration/spec.yaml` (config schema).

Scale: ~282k LOC of check code, ~331k LOC of tests, heavily skewed — a few DBM giants
(sqlserver, postgres, clickhouse, mysql: 6–16k LOC each) and a long tail of 200–1,500 LOC
data-driven checks.

**Goal:** the Agent core is being rewritten in Rust; every integration (logic + tests) must be
rewritten in Rust with byte-for-byte telemetry compatibility — same metric names, types, tags,
service checks, and config behavior — so no customer dashboard or monitor breaks.

## 2. What has already been done (all verified, on PR #1)

1. **Build/test baseline works.**
   - `ddev --no-interactive test pgbouncer`: 26 passed (docker compose integration suite).
   - `ddev test --lint pgbouncer`: clean.
   - `datadog_checks_base` core tests: 152 AgentCheck tests pass — these effectively define
     the framework contract the Rust port must honor.
2. **Rust proof-of-concept (`rust-poc/`)** — an isolated cargo workspace, not wired into any build:
   - `check-framework/`: minimal Rust `datadog_checks_base` — `AgentCheck` trait + a recording
     `Aggregator` mirroring the Python test stub (`assert_metric`, sorted-tag normalization,
     service checks).
   - `pgbouncer-check/`: full port of the pgbouncer integration. The `metrics.py` scope tables
     translate 1:1 into static Rust tables; row processing is pure/IO-free.
   - 8 unit tests porting the Python behavioral scenarios (config validation,
     `database_filter_regex`, SHOW CONFIG key/value flipping, internal-db skipping) — passing.
   - 2 live parity tests (`cargo test -p pgbouncer-check -- --ignored`) run against the
     **unchanged** Python docker compose fixture and assert the exact metric set + service
     check the Python integration test asserts — passing.
3. **Migration plan** (`MIGRATION_PLAN.md`): phasing, sequencing, bottlenecks, test strategy.

### Reproducing locally

```shell
# Python baseline (needs: libpq-dev libkrb5-dev libsasl2-dev libldap2-dev; Python 3.13 via uv; ddev)
ddev --no-interactive test pgbouncer

# Rust PoC (needs Rust >= 1.85; deps in the workspace already require it)
cd rust-poc && cargo test

# Live parity tests against the Python compose fixture
(cd pgbouncer/tests && POSTGRES_IMAGE_TAG=15.5 PGBOUNCER_IMAGE_TAG=1.23.1 \
   TEST_RESOURCES_PATH=$PWD/resources docker compose -f compose/docker-compose-v3.yml up -d)
cd rust-poc && cargo test -p pgbouncer-check -- --ignored
```

## 3. The plan in one paragraph

Don't big-bang. Phase 0: build the Rust `check-framework` (aggregator API + agent RPC
boundary, serde config generated from `spec.yaml`, HTTP wrapper, query executor, OpenMetrics
scraper, DBM job registry) plus a differential parity harness. Phase 1: the ~200 simple
tail checks — independent, massively parallelizable, one PR each, gated by parity tests
against the existing compose fixtures. Phase 2: framework families (OpenMetrics-based
checks, SNMP profile engine). Phase 3: the DBM giants (postgres, mysql, sqlserver,
clickhouse, kafka_consumer) last, with shadow-mode soak. Python and Rust checks run side by
side per integration behind a config flag until each is proven.

## 4. Challenges the engineer must be aware of

### Architecture / correctness
- **Agent RPC/ABI boundary (top bottleneck).** How a Rust check submits metrics to the Rust
  agent core is undefined until the agent team lands its check runner. Everything in Phase 0
  depends on its shape. Mitigation: code checks against the aggregator trait boundary (as the
  PoC does) so the transport can be swapped in later.
- **Rate semantics.** Python checks submit raw counters and the agent's aggregator computes
  per-second rates from successive submissions. The Rust agent core must own this — do not
  compute rates in checks. Not yet proven in the PoC.
- **Base-package fidelity.** Subtle behaviors define the telemetry contract: submission→backend
  metric-type mapping (rate→gauge, monotonic_count→count, histogram→multiple series), tag
  normalization, `is_affirmative`-style config coercion, HTTP wrapper auth/TLS/proxy edge
  cases. The 152 passing `datadog_checks_base` tests are the executable spec; port them as
  contract tests.
- **DBM async jobs.** `DBMAsyncJob` threads, the job registry, and the recently reworked
  shutdown/cancellation lifecycle must map onto tokio tasks with identical scheduling
  semantics (upstream PRs "keep DO scheduling state per query" define current behavior).
- **Config models.** Python uses pydantic models generated from `spec.yaml`. Generate serde
  structs from the same specs; never hand-write config structs, or drift is guaranteed.
- **Timestamp/gauge conversions** (e.g. pgbouncer `connect_time`/`request_time`) and other
  per-check quirks — omitted from the PoC, must be handled per integration.

### Dependencies
- **Native drivers.** tokio-postgres, mysql_async, tiberius, rdkafka are mature; but some
  checks depend on Python-only/vendored clients (ibm_mq, aerospike, vertica…). Inventory
  early; keep those on the Python runner until a Rust path exists.
- **SQL obfuscation** lives in the Go agent (`datadog-agent/pkg/obfuscate`); Phase 3 needs it
  exposed to Rust (FFI or rewrite).
- **Rust toolchain floor:** current crates already require Rust ≥ 1.85 (hit during the PoC —
  1.83 fails on `edition2024` deps). Pin toolchain + lockfile in CI from day one.

### Process
- **Upstream drift.** ~40+ PRs/month land in DataDog/integrations-core, much of it churn in
  the shared base (query_metrics primitives, OpenMetrics label handling, shutdown lifecycle).
  Long-lived port branches rot; only start an integration's port when it can merge within days.
- **Test discipline.** Port behavioral scenarios, not mechanics (repo rule — see AGENTS.md).
  The highest-value *new* test type is the Python-vs-Rust differential parity test: same live
  fixture, diff of (metric name, type, sorted tags) must be empty. The PoC's
  `pgbouncer-check/tests/integration.rs` is the seed; automate it in CI.
- **Contracts stay machine-checked.** Keep `metadata.csv`/`manifest.json`/`spec.yaml` as the
  source of truth; extend `ddev validate` to cover Rust crates (a Rust check's emitted-metric
  set must match metadata.csv both ways).
- **ddev integration.** New top-level non-integration directories must be added to
  `[overrides.is-integration]` in `.ddev/config.toml` (done for `rust-poc/`), or `ddev
  validate` treats them as integrations and fails on missing CHANGELOG/manifest.

### Environment gotchas (already hit and solved once)
- Hatch test envs need Python 3.13 on PATH (`uv python install 3.13`).
- Native build deps required: `libpq-dev libkrb5-dev libsasl2-dev libldap2-dev`.
- Older pgbouncer images have a c-ares DNS bug under Docker's embedded resolver; fixed via a
  `dns-search` entry in the Docker daemon config.
- Fork-only CI failures on this repo (`sohil-devin-org` fork): `labeler / apply` (dd-octo-sts
  403s for non-DataDog orgs) and `qa-label` (queries upstream PR numbers) fail on every PR
  regardless of content — ignore or patch the workflows.

## 5. Effort model

With Phase 0 done and the parity harness automated, each tail check is roughly one focused
AI session (port + tests + parity green), run in parallel. The framework and each DBM giant
are each a small number of focused sessions. The dominant costs are Phase 0 correctness and
the external wait on the Rust agent's RPC boundary — not code volume.
