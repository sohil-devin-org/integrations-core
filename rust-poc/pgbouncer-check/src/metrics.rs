//! Port of `pgbouncer/datadog_checks/pgbouncer/metrics.py`.

use check_framework::MetricType;

pub struct MetricScope {
    /// (row column, tag name)
    pub descriptors: &'static [(&'static str, &'static str)],
    /// (row column, (metric name, metric type))
    pub metrics: &'static [(&'static str, (&'static str, MetricType))],
    pub query: &'static str,
}

use MetricType::{Gauge, Rate};

pub const CONFIG_METRICS: MetricScope = MetricScope {
    descriptors: &[],
    metrics: &[("max_client_conn", ("pgbouncer.max_client_conn", Gauge))],
    query: "SHOW CONFIG",
};

pub const STATS_METRICS: MetricScope = MetricScope {
    descriptors: &[("database", "db")],
    metrics: &[
        (
            "total_requests",
            ("pgbouncer.stats.requests_per_second", Rate),
        ),
        (
            "total_xact_count",
            ("pgbouncer.stats.transactions_per_second", Rate),
        ),
        (
            "total_query_count",
            ("pgbouncer.stats.queries_per_second", Rate),
        ),
        (
            "total_received",
            ("pgbouncer.stats.bytes_received_per_second", Rate),
        ),
        (
            "total_sent",
            ("pgbouncer.stats.bytes_sent_per_second", Rate),
        ),
        (
            "total_query_time",
            ("pgbouncer.stats.total_query_time", Rate),
        ),
        (
            "total_xact_time",
            ("pgbouncer.stats.total_transaction_time", Rate),
        ),
        ("total_wait_time", ("pgbouncer.stats.total_wait_time", Rate)),
        ("avg_req", ("pgbouncer.stats.avg_req", Gauge)),
        (
            "avg_xact_count",
            ("pgbouncer.stats.avg_transaction_count", Gauge),
        ),
        (
            "avg_query_count",
            ("pgbouncer.stats.avg_query_count", Gauge),
        ),
        ("avg_wait_time", ("pgbouncer.stats.avg_wait_time", Gauge)),
        ("avg_recv", ("pgbouncer.stats.avg_recv", Gauge)),
        ("avg_sent", ("pgbouncer.stats.avg_sent", Gauge)),
        ("avg_query", ("pgbouncer.stats.avg_query", Gauge)),
        (
            "avg_xact_time",
            ("pgbouncer.stats.avg_transaction_time", Gauge),
        ),
        ("avg_query_time", ("pgbouncer.stats.avg_query_time", Gauge)),
        (
            "total_client_parse_count",
            ("pgbouncer.stats.client_parse_count_per_second", Rate),
        ),
        (
            "total_server_parse_count",
            ("pgbouncer.stats.server_parse_count_per_second", Rate),
        ),
        (
            "total_bind_count",
            ("pgbouncer.stats.bind_count_per_second", Rate),
        ),
        (
            "avg_client_parse_count",
            ("pgbouncer.stats.avg_client_parse_count", Gauge),
        ),
        (
            "avg_server_parse_count",
            ("pgbouncer.stats.avg_server_parse_count", Gauge),
        ),
        ("avg_bind_count", ("pgbouncer.stats.avg_bind_count", Gauge)),
    ],
    query: "SHOW STATS",
};

pub const POOLS_METRICS: MetricScope = MetricScope {
    descriptors: &[("database", "db"), ("user", "user")],
    metrics: &[
        ("cl_active", ("pgbouncer.pools.cl_active", Gauge)),
        ("cl_waiting", ("pgbouncer.pools.cl_waiting", Gauge)),
        ("sv_active", ("pgbouncer.pools.sv_active", Gauge)),
        ("sv_idle", ("pgbouncer.pools.sv_idle", Gauge)),
        ("sv_used", ("pgbouncer.pools.sv_used", Gauge)),
        ("sv_tested", ("pgbouncer.pools.sv_tested", Gauge)),
        ("sv_login", ("pgbouncer.pools.sv_login", Gauge)),
        ("maxwait", ("pgbouncer.pools.maxwait", Gauge)),
        ("maxwait_us", ("pgbouncer.pools.maxwait_us", Gauge)),
    ],
    query: "SHOW POOLS",
};

pub const DATABASES_METRICS: MetricScope = MetricScope {
    descriptors: &[
        ("name", "name"),
        ("name", "db"),
        ("database", "postgres_db"),
    ],
    metrics: &[
        ("pool_size", ("pgbouncer.databases.pool_size", Gauge)),
        (
            "max_connections",
            ("pgbouncer.databases.max_connections", Gauge),
        ),
        (
            "current_connections",
            ("pgbouncer.databases.current_connections", Gauge),
        ),
    ],
    query: "SHOW DATABASES",
};

pub const CLIENTS_METRICS: MetricScope = MetricScope {
    descriptors: &[("database", "db"), ("user", "user"), ("state", "state")],
    metrics: &[
        ("connect_time", ("pgbouncer.clients.connect_time", Gauge)),
        ("request_time", ("pgbouncer.clients.request_time", Gauge)),
        ("wait", ("pgbouncer.clients.wait", Gauge)),
        ("wait_us", ("pgbouncer.clients.wait_us", Gauge)),
        (
            "prepared_statements",
            ("pgbouncer.clients.prepared_statements", Gauge),
        ),
    ],
    query: "SHOW CLIENTS",
};

pub const SERVERS_METRICS: MetricScope = MetricScope {
    descriptors: &[("database", "db"), ("user", "user"), ("state", "state")],
    metrics: &[
        ("connect_time", ("pgbouncer.servers.connect_time", Gauge)),
        ("request_time", ("pgbouncer.servers.request_time", Gauge)),
        (
            "prepared_statements",
            ("pgbouncer.servers.prepared_statements", Gauge),
        ),
    ],
    query: "SHOW SERVERS",
};
