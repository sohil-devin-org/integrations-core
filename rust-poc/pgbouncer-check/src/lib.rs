//! Rust port of the PgBouncer integration
//! (`pgbouncer/datadog_checks/pgbouncer/pgbouncer.py`), written to de-risk the
//! Python -> Rust migration. Row-processing logic is separated from I/O so the
//! existing unit-test scenarios port directly; a live integration test runs
//! against the same docker compose environment as the Python suite.

pub mod metrics;

use std::collections::BTreeMap;

use check_framework::{AgentCheck, Aggregator, CheckError, ServiceCheckStatus};
use postgres::{Client, NoTls, SimpleQueryMessage};
use regex::Regex;

use metrics::{
    MetricScope, CLIENTS_METRICS, CONFIG_METRICS, DATABASES_METRICS, POOLS_METRICS,
    SERVERS_METRICS, STATS_METRICS,
};

pub const DB_NAME: &str = "pgbouncer";
pub const SERVICE_CHECK_NAME: &str = "pgbouncer.can_connect";

pub type Row = BTreeMap<String, String>;

#[derive(Debug, Default, Clone)]
pub struct PgBouncerConfig {
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub tags: Vec<String>,
    pub database_url: Option<String>,
    pub database_filter_regex: Option<String>,
    pub collect_per_client_metrics: bool,
    pub collect_per_server_metrics: bool,
}

pub struct PgBouncerCheck {
    config: PgBouncerConfig,
    database_filter: Option<Regex>,
    connection: Option<Client>,
}

impl PgBouncerCheck {
    pub fn new(config: PgBouncerConfig) -> Result<Self, CheckError> {
        let database_filter = match &config.database_filter_regex {
            Some(pattern) if !pattern.is_empty() => Some(Regex::new(pattern).map_err(|e| {
                CheckError::Configuration(format!("Invalid database_filter_regex: {e}"))
            })?),
            _ => None,
        };

        if config.database_url.is_none() {
            if config.host.is_empty() {
                return Err(CheckError::Configuration(
                    "Please specify a PgBouncer host to connect to.".into(),
                ));
            }
            if config.username.is_empty() {
                return Err(CheckError::Configuration(
                    "Please specify a user to connect to PgBouncer as.".into(),
                ));
            }
        }

        Ok(Self {
            config,
            database_filter,
            connection: None,
        })
    }

    fn service_check_tags(&self) -> Vec<String> {
        let (host, port) = match &self.config.database_url {
            // Minimal URL parsing; production code would use the `url` crate.
            Some(url) => parse_host_port(url),
            None => (self.config.host.clone(), self.config.port.clone()),
        };
        let mut tags = vec![
            format!("host:{host}"),
            format!("port:{port}"),
            format!("db:{DB_NAME}"),
        ];
        tags.extend(self.config.tags.iter().cloned());
        tags.sort();
        tags.dedup();
        tags
    }

    fn conninfo(&self) -> String {
        if let Some(url) = &self.config.database_url {
            return url.clone();
        }
        let mut parts = vec![
            format!("host={}", self.config.host),
            format!("user={}", self.config.username),
            format!("dbname={DB_NAME}"),
        ];
        if !self.config.port.is_empty() {
            parts.push(format!("port={}", self.config.port));
        }
        if !self.config.password.is_empty() {
            parts.push(format!("password={}", self.config.password));
        }
        parts.join(" ")
    }

    fn ensure_connection(&mut self) -> Result<&mut Client, CheckError> {
        if self.connection.is_none() {
            let client = Client::connect(&self.conninfo(), NoTls)
                .map_err(|e| CheckError::Runtime(format!("Cannot establish connection: {e}")))?;
            self.connection = Some(client);
        }
        Ok(self.connection.as_mut().unwrap())
    }

    fn scopes(&self) -> Vec<&'static MetricScope> {
        let mut scopes = vec![
            &STATS_METRICS,
            &POOLS_METRICS,
            &DATABASES_METRICS,
            &CONFIG_METRICS,
        ];
        if self.config.collect_per_client_metrics {
            scopes.push(&CLIENTS_METRICS);
        }
        if self.config.collect_per_server_metrics {
            scopes.push(&SERVERS_METRICS);
        }
        scopes
    }

    fn collect_stats(&mut self, aggregator: &Aggregator) -> Result<(), CheckError> {
        let scopes = self.scopes();
        let tags = self.config.tags.clone();
        let filter = self.database_filter.clone();
        let client = self.ensure_connection()?;

        for scope in scopes {
            let messages = client
                .simple_query(scope.query)
                .map_err(|e| CheckError::Runtime(format!("query {} failed: {e}", scope.query)))?;
            let rows = simple_query_rows(messages);
            process_rows(scope, &rows, &tags, filter.as_ref(), aggregator);
        }
        Ok(())
    }
}

impl AgentCheck for PgBouncerCheck {
    fn run(&mut self, aggregator: &Aggregator) -> Result<(), CheckError> {
        match self.collect_stats(aggregator) {
            Ok(()) => {
                aggregator.submit_service_check(
                    SERVICE_CHECK_NAME,
                    ServiceCheckStatus::Ok,
                    &self.service_check_tags(),
                    "",
                );
                Ok(())
            }
            Err(e) => {
                // Reconnect-once semantics of the Python check collapse to a
                // reset here; the next run re-establishes the connection.
                self.connection = None;
                aggregator.submit_service_check(
                    SERVICE_CHECK_NAME,
                    ServiceCheckStatus::Critical,
                    &self.service_check_tags(),
                    &e.to_string(),
                );
                Err(e)
            }
        }
    }
}

fn parse_host_port(url: &str) -> (String, String) {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let authority = after_scheme.split('/').next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let mut it = host_port.split(':');
    (
        it.next().unwrap_or("").to_string(),
        it.next().unwrap_or("").to_string(),
    )
}

/// Convert simple-query protocol results (all values arrive as text, which is
/// exactly what the PgBouncer admin console speaks) into name->value rows.
pub fn simple_query_rows(messages: Vec<SimpleQueryMessage>) -> Vec<Row> {
    let mut rows = Vec::new();
    for message in messages {
        if let SimpleQueryMessage::Row(row) = message {
            let mut map = Row::new();
            for i in 0..row.len() {
                if let Some(value) = row.get(i) {
                    map.insert(row.columns()[i].name().to_string(), value.to_string());
                }
            }
            rows.push(map);
        }
    }
    rows
}

fn row_database_name(row: &Row) -> Option<&String> {
    row.get("name").or_else(|| row.get("database"))
}

pub fn should_collect_row(row: &Row, filter: Option<&Regex>) -> bool {
    let Some(filter) = filter else { return true };
    match row_database_name(row) {
        Some(name) => filter.is_match(name),
        None => true,
    }
}

/// Core row-to-metric mapping, kept pure for unit testing. Mirrors the body of
/// `PgBouncer._collect_stats`.
pub fn process_rows(
    scope: &MetricScope,
    rows: &[Row],
    base_tags: &[String],
    filter: Option<&Regex>,
    aggregator: &Aggregator,
) {
    for row in rows {
        let mut row = row.clone();
        if let Some(key) = row.get("key").cloned() {
            // SHOW CONFIG rows arrive as (key, value) pairs; flip them so the
            // config name becomes a column.
            if let Some(value) = row.get("value").cloned() {
                row.insert(key, value);
            }
        } else if row.get("database").map(String::as_str) == Some(DB_NAME) {
            continue;
        }

        if !should_collect_row(&row, filter) {
            continue;
        }

        let mut tags: Vec<String> = base_tags.to_vec();
        for (column, tag) in scope.descriptors {
            if let Some(value) = row.get(*column) {
                tags.push(format!("{tag}:{value}"));
            }
        }

        for (column, (name, metric_type)) in scope.metrics {
            if let Some(raw) = row.get(*column) {
                // connect_time/request_time timestamp parsing is omitted in
                // this PoC; those columns are only used by the optional
                // clients/servers scopes.
                if let Ok(value) = raw.parse::<f64>() {
                    aggregator.submit_metric(name, *metric_type, value, &tags);
                }
            }
        }
    }
}
