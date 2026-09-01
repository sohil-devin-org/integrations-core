//! Live parity test against the same docker compose environment the Python
//! suite uses (`pgbouncer/tests/compose`). Run with:
//!
//! ```shell
//! (cd pgbouncer/tests && POSTGRES_IMAGE_TAG=16 PGBOUNCER_IMAGE_TAG=1.12.0 \
//!    TEST_RESOURCES_PATH=$PWD/resources docker compose -f compose/docker-compose-v2.yml up -d)
//! cargo test -p pgbouncer-check -- --ignored
//! ```

use check_framework::{AgentCheck, Aggregator, ServiceCheckStatus};
use pgbouncer_check::{PgBouncerCheck, PgBouncerConfig, SERVICE_CHECK_NAME};

fn live_config() -> PgBouncerConfig {
    PgBouncerConfig {
        host: "localhost".into(),
        port: "6432".into(),
        username: "postgres".into(),
        password: "d@tadog".into(),
        tags: vec!["optional:tag1".into()],
        ..Default::default()
    }
}

/// Mirrors `test_check` in `test_pgbouncer_integration_e2e.py`: the metrics the
/// Python check asserts against a live pgbouncer must also be emitted by the
/// Rust port, and the service check must be OK.
#[test]
#[ignore = "requires the pgbouncer docker compose environment"]
fn live_check_emits_python_parity_metrics() {
    // Mirror the Python suite: generate traffic through pgbouncer so pools /
    // stats rows exist for the datadog_test database.
    let mut app = postgres::Client::connect(
        "host=localhost port=6432 user=postgres password=d@tadog dbname=datadog_test",
        postgres::NoTls,
    )
    .expect("cannot connect through pgbouncer");
    app.simple_query("SELECT 1").unwrap();

    let mut check = PgBouncerCheck::new(live_config()).unwrap();
    let aggregator = Aggregator::new();
    check.run(&aggregator).expect("check run failed");

    // Metric set asserted by the Python integration test (v2/1.12 era columns).
    for name in [
        "pgbouncer.pools.cl_active",
        "pgbouncer.pools.cl_waiting",
        "pgbouncer.pools.sv_active",
        "pgbouncer.pools.sv_idle",
        "pgbouncer.pools.sv_used",
        "pgbouncer.pools.sv_tested",
        "pgbouncer.pools.sv_login",
        "pgbouncer.pools.maxwait",
        "pgbouncer.stats.avg_recv",
        "pgbouncer.stats.avg_sent",
        "pgbouncer.stats.bytes_received_per_second",
        "pgbouncer.stats.bytes_sent_per_second",
        "pgbouncer.databases.pool_size",
        "pgbouncer.databases.max_connections",
        "pgbouncer.databases.current_connections",
        "pgbouncer.max_client_conn",
    ] {
        aggregator.assert_metric(name, None);
    }

    let service_checks = aggregator.service_checks(SERVICE_CHECK_NAME);
    assert!(!service_checks.is_empty());
    assert!(service_checks
        .iter()
        .all(|s| s.status == ServiceCheckStatus::Ok));
}

#[test]
#[ignore = "requires the pgbouncer docker compose environment"]
fn live_check_critical_service_check_on_bad_port() {
    let mut config = live_config();
    config.port = "7000".into();
    let mut check = PgBouncerCheck::new(config).unwrap();
    let aggregator = Aggregator::new();
    assert!(check.run(&aggregator).is_err());

    let service_checks = aggregator.service_checks(SERVICE_CHECK_NAME);
    assert!(!service_checks.is_empty());
    assert!(service_checks
        .iter()
        .all(|s| s.status == ServiceCheckStatus::Critical));
}
