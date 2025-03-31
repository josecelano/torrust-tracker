use std::collections::BTreeMap;

use serde::Serialize;
use torrust_tracker_primitives::metrics::LabeledMetrics;

/// Metrics collected by the tracker.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Metrics {
    /// Total number of TCP (HTTP tracker) `announce` requests from IPv4 peers.
    pub tcp4_announces_handled: u64,

    /// Total number of TCP (HTTP tracker) `scrape` requests from IPv4 peers.
    pub tcp4_scrapes_handled: u64,

    /// Total number of TCP (HTTP tracker) `announce` requests from IPv6 peers.
    pub tcp6_announces_handled: u64,

    /// Total number of TCP (HTTP tracker) `scrape` requests from IPv6 peers.
    pub tcp6_scrapes_handled: u64,

    pub labeled_metrics: LabeledMetrics,
}

impl Metrics {
    pub fn increase_counter(&mut self, metric_name: &str, metric_labels: &BTreeMap<String, String>) {
        self.labeled_metrics.increase_counter(metric_name, metric_labels);
    }
}
