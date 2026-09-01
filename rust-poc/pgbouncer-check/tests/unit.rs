//! Ports of the behavioral scenarios in `pgbouncer/tests/test_pgbouncer_unit.py`.

use check_framework::{Aggregator, CheckError, MetricType};
use pgbouncer_check::metrics::{DATABASES_METRICS, POOLS_METRICS, STATS_METRICS};
use pgbouncer_check::{process_rows, PgBouncerCheck, PgBouncerConfig, Row};
use regex::Regex;

fn row(pairs: &[(&str, &str)]) -> Row {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn config_missing_host_is_rejected() {
    let err = PgBouncerCheck::new(PgBouncerConfig {
        username: "postgres".into(),
        ..Default::default()
    })
    .err()
    .expect("missing host must be a configuration error");
    assert!(matches!(err, CheckError::Configuration(_)));
}

#[test]
fn config_missing_user_is_rejected() {
    let err = PgBouncerCheck::new(PgBouncerConfig {
        host: "localhost".into(),
        ..Default::default()
    })
    .err()
    .expect("missing user must be a configuration error");
    assert!(matches!(err, CheckError::Configuration(_)));
}

#[test]
fn config_invalid_database_filter_regex_is_rejected() {
    let err = PgBouncerCheck::new(PgBouncerConfig {
        host: "localhost".into(),
        username: "postgres".into(),
        database_filter_regex: Some("(unclosed".into()),
        ..Default::default()
    })
    .err()
    .expect("invalid regex must be a configuration error");
    assert!(matches!(err, CheckError::Configuration(_)));
}

#[test]
fn database_filter_drops_non_matching_stats_rows() {
    let aggregator = Aggregator::new();
    let filter = Regex::new("^datadog_test$").unwrap();
    let rows = vec![
        row(&[("database", "datadog_test"), ("total_query_count", "10")]),
        row(&[("database", "other_db"), ("total_query_count", "99")]),
    ];
    process_rows(&STATS_METRICS, &rows, &[], Some(&filter), &aggregator);

    let samples = aggregator.metrics("pgbouncer.stats.queries_per_second");
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].tags, vec!["db:datadog_test"]);
}

#[test]
fn database_filter_matches_show_databases_name_column() {
    let aggregator = Aggregator::new();
    let filter = Regex::new("^datadog_test$").unwrap();
    let rows = vec![
        row(&[
            ("name", "datadog_test"),
            ("database", "datadog_test"),
            ("pool_size", "5"),
        ]),
        row(&[
            ("name", "other_db"),
            ("database", "other_db"),
            ("pool_size", "7"),
        ]),
    ];
    process_rows(&DATABASES_METRICS, &rows, &[], Some(&filter), &aggregator);

    let samples = aggregator.metrics("pgbouncer.databases.pool_size");
    assert_eq!(samples.len(), 1);
    assert!(samples[0].tags.contains(&"name:datadog_test".to_string()));
}

#[test]
fn database_filter_keeps_rows_without_database_column() {
    let aggregator = Aggregator::new();
    let filter = Regex::new("^will_not_match$").unwrap();
    // SHOW CONFIG rows have no database column and must never be filtered.
    let rows = vec![row(&[("key", "max_client_conn"), ("value", "100")])];
    process_rows(
        &pgbouncer_check::metrics::CONFIG_METRICS,
        &rows,
        &[],
        Some(&filter),
        &aggregator,
    );
    aggregator.assert_metric("pgbouncer.max_client_conn", None);
}

#[test]
fn config_rows_are_flipped_into_columns() {
    let aggregator = Aggregator::new();
    let rows = vec![
        row(&[("key", "max_client_conn"), ("value", "42")]),
        row(&[("key", "unrelated_setting"), ("value", "1")]),
    ];
    process_rows(
        &pgbouncer_check::metrics::CONFIG_METRICS,
        &rows,
        &[],
        None,
        &aggregator,
    );

    let samples = aggregator.metrics("pgbouncer.max_client_conn");
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].value, 42.0);
    assert_eq!(samples[0].metric_type, MetricType::Gauge);
}

#[test]
fn pgbouncer_internal_database_rows_are_skipped() {
    let aggregator = Aggregator::new();
    let rows = vec![
        row(&[
            ("database", "pgbouncer"),
            ("cl_active", "3"),
            ("user", "postgres"),
        ]),
        row(&[
            ("database", "datadog_test"),
            ("cl_active", "5"),
            ("user", "postgres"),
        ]),
    ];
    process_rows(
        &POOLS_METRICS,
        &rows,
        &["optional:tag1".into()],
        None,
        &aggregator,
    );

    let samples = aggregator.metrics("pgbouncer.pools.cl_active");
    assert_eq!(samples.len(), 1);
    let mut expected = vec![
        "db:datadog_test".to_string(),
        "optional:tag1".to_string(),
        "user:postgres".to_string(),
    ];
    expected.sort();
    assert_eq!(samples[0].tags, expected);
}
