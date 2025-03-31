use std::collections::BTreeMap;

use serde::Serialize;

/// Metrics collected by the tracker.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct LabeledMetrics {
    pub metrics: Vec<LabeledMetric>,
}

impl LabeledMetrics {
    pub fn increase_counter(&mut self, metric_name: &str, metric_labels: &BTreeMap<String, String>) {
        let mut found = false;

        for labeled_metric in &mut self.metrics {
            // todo:
            //   - Check that the metric has the counter type.

            if labeled_metric.metric.name == metric_name && labeled_metric.labels == *metric_labels {
                labeled_metric.metric.value += 1;
                found = true;
                break;
            }
        }

        if !found {
            self.metrics.push(LabeledMetric {
                metric: Metric {
                    name: metric_name.to_string(),
                    kind: "counter".to_string(),
                    value: 1,
                },
                labels: metric_labels.clone(),
            });
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize)]
pub struct LabeledMetric {
    pub metric: Metric,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize)]
pub struct Metric {
    pub name: String,
    pub kind: String,
    pub value: u64, // todo: change to f64. See https://prometheus.io/docs/concepts/data_model/#samples
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::LabeledMetric;
    use crate::metrics::{LabeledMetrics, Metric};

    #[allow(clippy::no_effect_replace)]
    #[test]
    fn metrics_should_be_serializable_to_json() {
        let metrics = LabeledMetrics {
            metrics: vec![LabeledMetric {
                metric: Metric {
                    name: "announce_requests_received_total".to_string(),
                    kind: "counter".to_string(),
                    value: 325,
                },
                labels: BTreeMap::from([
                    ("ip_version".to_string(), "ipv4".to_string()),
                    ("protocol".to_string(), "udp".to_string()),
                    ("url".to_string(), "udp://127.0.0.1:6969".to_string()),
                ]),
            }],
        };

        let json = serde_json::to_string(&metrics).unwrap();

        assert_eq!(
            formatjson::format_json(&json).unwrap(),
            formatjson::format_json(
                r#"
                {
                    "metrics": [
                        {
                            "metric": {
                                "name": "announce_requests_received_total",
                                "kind": "counter",
                                "value": 325
                            },
                            "labels": {
                                "ip_version":"ipv4",
                                "protocol":"udp",
                                "url":"udp://127.0.0.1:6969"
                            }
                        }
                    ]
                }"#
            )
            .unwrap()
        );
    }
}
