# Python → Rust Migration Plan for integrations-core

This plan is grounded in three things done in this repo: (1) building and running the
existing Python test/tooling stack (`ddev`, hatch, docker compose E2E), (2) surveying
recent upstream DataDog/integrations-core commits and PRs, and (3) a working Rust
proof-of-concept (`rust-poc/`) that ports the PgBouncer check with unit tests and a
live parity test against the same docker compose environment the Python suite uses.

## 1. What the repo actually is (scale and shape)

- ~265 integrations with collection logic (~282k LOC of check code, ~331k LOC of tests),
  plus tiles with no logic. `datadog_checks_base` (~25k LOC) is the shared framework
  every check depends on; `datadog_checks_dev`/`ddev` (~21k LOC) is the test/dev harness.
- Distribution is extremely skewed: a handful of DBM integrations (sqlserver, postgres,
  clickhouse, mysql) are 6–16k LOC each with async job scheduling, obfuscation, and
  statement sampling; the long tail is 200–1,500 LOC checks that are data-driven
  (query → rows → metrics, or HTTP/OpenMetrics scraping).
- Upstream commit history shows heavy ongoing investment in *shared* primitives
  (query_metrics primitives, DBM job registry/cancellation lifecycle, metric-submission
  fast paths, OpenMetrics label handling). Integrations increasingly delegate to the
  base package — which is good news: migrating the base faithfully migrates most of the
  behavior of hundreds of checks at once.

## 2. What the PoC proved (and what it didn't)

Proved (all tests passing in `rust-poc/`):

- The `AgentCheck`/`Aggregator` contract ports cleanly to a Rust trait + recording
  aggregator, preserving the exact test semantics of the Python stub
  (`assert_metric`, tag normalization, service-check recording).
- Data-driven metric scopes (`metrics.py` tables) translate 1:1 into static Rust
  tables — the metric-name/type/tag contract is preserved mechanically.
- Row-processing logic isolated from I/O lets Python unit-test scenarios port
  directly (config validation, database filtering, SHOW CONFIG row flipping,
  internal-db skipping).
- The Rust check runs against the *unchanged* Python docker compose environment and
  emits the exact metric set the Python integration test asserts, with an OK service
  check — i.e., the existing E2E infrastructure is reusable as-is for parity testing.

Not yet proved (known gaps to de-risk next):

- Rates: the Python agent computes per-second rates from successive submissions in
  the Go aggregator; the Rust agent core must own this, not the check.
- Timestamp→gauge conversions (connect_time/request_time), config models
  (pydantic-generated), autodiscovery, DBM async jobs, and the RPC boundary between
  checks and the Rust agent core.

## 3. Target architecture

1. **`check-framework` crate** — Rust `datadog_checks_base`: `AgentCheck` trait,
   aggregator client (metrics/service checks/events/metadata), config
   deserialization (serde replaces pydantic models, generated from the same
   `spec.yaml` files), HTTP wrapper with the same auth/TLS/proxy options, DB query
   executor (the `QueryManager`/query_metrics analog), OpenMetrics scraper, log
   support, and the DBM job registry (tokio tasks replace `DBMAsyncJob` threads —
   upstream's recent shutdown/cancellation work defines the exact lifecycle to match).
2. **One crate per integration**, mirroring today's one-directory-per-integration
   layout, keeping `metadata.csv`, `manifest.json`, and `spec.yaml` as the unchanged
   source-of-truth contracts (they are language-neutral and validated by `ddev`).
3. **A conformance/test crate** mirroring the Python aggregator stub so ported tests
   keep their assertions, plus a *differential harness* that runs the Python check and
   the Rust check against the same live fixture and diffs (name, type, sorted tags)
   tuples — the PoC's parity test is the seed of this.

## 4. Work split and sequencing

**Phase 0 — Foundation (blocks everything, do first, small team/serial):**
- check-framework crate: aggregator API + agent RPC boundary (must be co-designed
  with the Rust agent team — this is the #1 external dependency/bottleneck).
- Config generation from `spec.yaml` → serde structs (replaces config_models).
- Test stubs + differential harness + CI wiring (`ddev` gains `test --rust` and the
  compose/E2E reuse shown in the PoC).

**Phase 1 — Tail integrations (massively parallel, ideal for AI-driven migration):**
- ~200 checks that are pure data-mapping or simple HTTP/DB scrapers (pgbouncer-class).
  Each is an independent unit: port `metrics.py`-style tables, port check logic, port
  unit tests, run differential parity against the existing compose env. No cross-check
  dependencies → parallelize by integration, one PR each, machine-verifiable via the
  parity harness.

**Phase 2 — Framework-heavy families (parallel by family):**
- OpenMetrics-based checks (kube_*, envoy, istio, cilium…): once the Rust OpenMetrics
  scraper matches Python behavior (label sharing, target_info, metric limits — all
  active upstream churn), these ports are mostly config.
- SNMP: profiles are YAML (language-neutral); port the profile engine once.
- JMX-based checks are already out-of-process (jmxfetch) — no port needed initially.

**Phase 3 — DBM giants (serial-ish, expert review):**
- postgres, mysql, sqlserver, clickhouse, kafka_consumer. These need native drivers
  (tokio-postgres, mysql_async, tiberius, rdkafka — all mature), the DBM job
  registry, obfuscation (already in Go in the agent; expose to Rust or rewrite), and
  statement-metrics state. Do last, with the longest parity soak.

**Rollout/coexistence:** the agent runs Python and Rust checks side by side per
integration (config flag), enabling canary + shadow mode (run both, diff series in
telemetry) before flipping defaults. Never a big-bang cutover.

## 5. Bottlenecks and risks

1. **The agent RPC/ABI for checks** — undefined until the Rust agent core lands its
   check runner; everything in Phase 0 depends on its shape. Mitigate by building
   against the recording-aggregator boundary (as the PoC does) so check code is
   agnostic to the transport.
2. **Behavioral fidelity of the base package** — metric type semantics (rate vs
   gauge submission→backend mapping), tag normalization, `is_affirmative`-style
   config coercion, HTTP wrapper edge cases. Mitigate with contract tests extracted
   from `datadog_checks_base` tests (152 core AgentCheck tests currently pass and
   define the contract) and the differential harness.
3. **Upstream drift** — ~40+ PRs/month land upstream; long-lived ports rot. Mitigate
   by migrating an integration only when its port can merge within days, and by
   freezing (or dual-maintaining) only the integration currently being ported.
4. **Native drivers/dependencies** — a few checks depend on Python-only or vendored
   libs (e.g., ibm_mq, aerospike, vertica clients). Inventory early; some become
   "keep on Python runner" until a Rust/C driver path exists.
5. **Rust toolchain floor** — current crates.io deps already require Rust ≥1.85
   (hit in the PoC); pin toolchain + lockfile in CI from day one.

## 6. Testing strategy (end-to-end guarantees)

- **Keep the contracts machine-checked:** `metadata.csv` stays the metric contract;
  add a CI check that every metric a Rust check can emit appears in metadata.csv and
  vice versa (Python has this via `ddev validate`; extend to Rust).
- **Port behavioral unit tests, not mechanics** (per AGENTS.md test rules): the PoC
  shows scenario-level tests (filtering, config errors, row handling) port directly.
- **Reuse the existing E2E infra unchanged:** compose files, fixtures, and `ddev env`
  already work for Rust binaries (proved in the PoC). Add `ddev env test --rust`.
- **Differential/parity tests are the acceptance gate:** same live fixture, Python
  vs Rust, diff of (metric, type, sorted tags) sets must be empty (modulo an explicit
  allowlist). This is the test we should *add* — it doesn't exist today and is the
  single highest-value new test type for this migration.
- **Shadow mode in production** as the final gate for tier-1 integrations.

## 7. Estimated effort

With the Phase 0 framework in place and the parity harness automated, tail
integrations (Phase 1) are highly parallelizable AI sessions: roughly one session per
small check (port + tests + parity green). The framework itself and each DBM giant
are each a small number of focused sessions. The dominant cost is not code volume but
Phase 0 correctness and the RPC dependency on the Rust agent core team.

## 8. Useful external tools/infra identified from upstream

- GitHub Actions matrix + labels (`qa/*`, `team/*`) — reuse for `rust/` label lanes.
- Docker compose fixtures per integration (reused as-is by the PoC).
- `ddev` — extend rather than replace; it owns validation of the language-neutral
  contracts (manifest, metadata, config specs, changelogs, deps).
- Obfuscation lives in the Go agent (`datadog-agent/pkg/obfuscate`) — candidate for a
  shared Rust crate or FFI, needed only in Phase 3.
- crates: tokio-postgres/postgres, mysql_async, tiberius, rdkafka, hyper/reqwest,
  prometheus-parse (or custom OpenMetrics parser for fidelity), serde/serde_yaml.
