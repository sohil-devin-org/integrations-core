//! Minimal Rust equivalent of `datadog_checks.base`: the `AgentCheck` contract
//! plus a recording `Aggregator` that mirrors the Python test stub
//! (`datadog_checks_base/datadog_checks/base/stubs/aggregator.py`) so ported
//! integrations can keep their existing test semantics.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MetricType {
    Gauge,
    Rate,
    Count,
    MonotonicCount,
    Histogram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCheckStatus {
    Ok = 0,
    Warning = 1,
    Critical = 2,
    Unknown = 3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricSample {
    pub name: String,
    pub metric_type: MetricType,
    pub value: f64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceCheckSample {
    pub name: String,
    pub status: ServiceCheckStatus,
    pub tags: Vec<String>,
    pub message: String,
}

#[derive(Debug)]
pub enum CheckError {
    Configuration(String),
    Runtime(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Configuration(msg) => write!(f, "ConfigurationError: {msg}"),
            CheckError::Runtime(msg) => write!(f, "CheckError: {msg}"),
        }
    }
}

impl std::error::Error for CheckError {}

/// Sink for check output. Production would forward to the agent core;
/// tests use it directly as a recording stub.
#[derive(Default)]
pub struct Aggregator {
    metrics: Mutex<Vec<MetricSample>>,
    service_checks: Mutex<Vec<ServiceCheckSample>>,
}

impl Aggregator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit_metric(&self, name: &str, metric_type: MetricType, value: f64, tags: &[String]) {
        let mut tags = tags.to_vec();
        tags.sort();
        self.metrics.lock().unwrap().push(MetricSample {
            name: name.to_string(),
            metric_type,
            value,
            tags,
        });
    }

    pub fn submit_service_check(
        &self,
        name: &str,
        status: ServiceCheckStatus,
        tags: &[String],
        message: &str,
    ) {
        let mut tags = tags.to_vec();
        tags.sort();
        self.service_checks
            .lock()
            .unwrap()
            .push(ServiceCheckSample {
                name: name.to_string(),
                status,
                tags,
                message: message.to_string(),
            });
    }

    pub fn metrics(&self, name: &str) -> Vec<MetricSample> {
        self.metrics
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.name == name)
            .cloned()
            .collect()
    }

    pub fn metric_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .metrics
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn service_checks(&self, name: &str) -> Vec<ServiceCheckSample> {
        self.service_checks
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.name == name)
            .cloned()
            .collect()
    }

    /// Mirrors the Python stub's `assert_metric`: at least one sample with the
    /// given name (and tags, when provided) must exist.
    pub fn assert_metric(&self, name: &str, tags: Option<&[String]>) {
        let samples = self.metrics(name);
        assert!(!samples.is_empty(), "metric {name} was not submitted");
        if let Some(tags) = tags {
            let mut expected = tags.to_vec();
            expected.sort();
            assert!(
                samples.iter().any(|s| s.tags == expected),
                "metric {name} found but not with tags {expected:?}; got {:?}",
                samples.iter().map(|s| &s.tags).collect::<Vec<_>>()
            );
        }
    }

    pub fn metrics_by_type(&self) -> BTreeMap<String, MetricType> {
        self.metrics
            .lock()
            .unwrap()
            .iter()
            .map(|m| (m.name.clone(), m.metric_type))
            .collect()
    }
}

/// The check contract: one `run` per collection interval, submitting through
/// the aggregator. Mirrors `AgentCheck.check(instance)`.
pub trait AgentCheck {
    fn run(&mut self, aggregator: &Aggregator) -> Result<(), CheckError>;
}
