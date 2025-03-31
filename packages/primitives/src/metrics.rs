use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize, Hash, Ord, PartialOrd)]
pub struct MetricName(String);

impl MetricName {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }
}

/// Metrics collected by the tracker.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct LabeledMetricCollection {
    metrics: BTreeMap<MetricName, LabeledMetric>,
}

impl LabeledMetricCollection {
    #[must_use]
    pub fn get(&self, metric_name: &MetricName, labels: &LabelSet) -> Option<MetricSample> {
        match self.metrics.get(metric_name) {
            Some(labeled_metric) => match labeled_metric.values.get(labels) {
                Some(metric_value) => Some(MetricSample {
                    value: metric_value.value,
                    update_at: metric_value.update_at.clone(),
                    labels: labels.clone(),
                }),
                None => None,
            },
            None => None,
        }
    }

    pub fn increase_counter(&mut self, metric_name: &MetricName, labels: &LabelSet) {
        self.metrics
            .entry(metric_name.clone())
            .or_insert_with(|| LabeledMetric {
                name: metric_name.clone(),
                kind: "counter".to_string(),
                values: MetricValueVec::default(),
            })
            .values
            .increase_counter(labels);
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize)]
pub struct LabeledMetric {
    pub name: MetricName,
    pub kind: String,
    pub values: MetricValueVec,
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize)]
pub struct MetricValueVec {
    values: Vec<MetricSample>,
}

impl MetricValueVec {
    #[must_use]
    pub fn get(&self, labels: &LabelSet) -> Option<MetricSample> {
        self.values.iter().find(|value| value.labels == *labels).cloned()
    }

    pub fn increase_counter(&mut self, labels: &LabelSet) {
        for value in &mut self.values {
            if value.labels == *labels {
                value.increase();
                return;
            }
        }

        // If no value was found for the given labels, create a new one
        let new_value = MetricSample {
            value: 1,
            update_at: "now".to_string(), // todo: use a real timestamp
            labels: labels.clone(),
        };

        self.values.push(new_value);
    }
}

impl From<Vec<MetricSample>> for MetricValueVec {
    fn from(values: Vec<MetricSample>) -> Self {
        Self { values }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize)]
pub struct MetricSample {
    pub value: u64,        // todo: change to f64. See https://prometheus.io/docs/concepts/data_model/#samples
    pub update_at: String, // todo:  use type
    pub labels: LabelSet,
}

impl MetricSample {
    pub fn increase(&mut self) {
        // todo: update time
        self.value += 1;
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize, Hash, Ord, PartialOrd)]
pub struct LabelName(String);

impl LabelName {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize, Hash, Ord, PartialOrd)]
pub struct LabelValue(String);

impl LabelValue {
    #[must_use]
    pub fn new(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize)]
pub struct LabelSet {
    #[serde(flatten)]
    set: BTreeMap<LabelName, LabelValue>,
}

impl LabelSet {
    #[must_use]
    pub fn new(set: BTreeMap<LabelName, LabelValue>) -> Self {
        Self { set }
    }

    pub fn set(&mut self, key: LabelName, value: LabelValue) {
        self.set.insert(key, value);
    }
}

impl From<(LabelName, LabelValue)> for LabelSet {
    fn from(label_pair: (LabelName, LabelValue)) -> Self {
        let mut set = BTreeMap::new();

        set.insert(label_pair.0, label_pair.1);

        Self { set }
    }
}

impl From<Vec<(LabelName, LabelValue)>> for LabelSet {
    fn from(vec: Vec<(LabelName, LabelValue)>) -> Self {
        let mut set = BTreeMap::new();

        for (key, value) in vec {
            set.insert(key, value);
        }

        Self { set }
    }
}

#[cfg(test)]
mod tests {

    mod a_labeled_metric_collection {
        use crate::metrics::{LabelName, LabelSet, LabelValue, LabeledMetricCollection, MetricName};

        #[test]
        fn should_allow_increase_a_counter_metric_value_for_an_specific_label_set() {
            let mut labeled_metric_collection = LabeledMetricCollection::default();

            let metric_name = MetricName::new("announce_requests_received_total");
            let label_set: LabelSet = vec![(LabelName::new("key 1"), LabelValue::new("value 1"))].into();

            labeled_metric_collection.increase_counter(&metric_name, &label_set);

            assert_eq!(labeled_metric_collection.get(&metric_name, &label_set).unwrap().value, 1);
        }
    }

    mod metric_value_vec {
        use crate::metrics::{LabelName, LabelSet, LabelValue, MetricSample, MetricValueVec};

        #[test]
        fn should_allow_increase_a_counter_metric_value_for_an_specific_label_set() {
            let label_set: LabelSet = vec![(LabelName::new("key 1"), LabelValue::new("value 1"))].into();

            let mut metric_value_vec = MetricValueVec {
                values: vec![MetricSample {
                    value: 0,
                    update_at: "now".to_string(),
                    labels: label_set.clone(),
                }],
            };

            metric_value_vec.increase_counter(&label_set);

            assert_eq!(metric_value_vec.get(&label_set).unwrap().value, 1);
        }

        #[test]
        fn should_return_a_metric_value_for_an_specific_label_set() {
            let label_set: LabelSet = vec![(LabelName::new("key 1"), LabelValue::new("value 1"))].into();

            let metric_value_vec = MetricValueVec {
                values: vec![
                    MetricSample {
                        value: 1,
                        update_at: "now".to_string(),
                        labels: label_set.clone(),
                    },
                    MetricSample {
                        value: 2,
                        update_at: "now".to_string(),
                        labels: vec![(LabelName::new("key 2"), LabelValue::new("value 2"))].into(),
                    },
                ],
            };

            assert_eq!(
                metric_value_vec.get(&label_set),
                Some(MetricSample {
                    value: 1,
                    update_at: "now".to_string(),
                    labels: vec![(LabelName::new("key 1"), LabelValue::new("value 1"))].into(),
                })
            );
        }
    }

    mod metric_value {
        use super::super::MetricSample;
        use crate::metrics::LabelSet;

        #[test]
        fn could_be_increased() {
            let mut metric_value = MetricSample {
                value: 0,
                update_at: "now".to_string(),
                labels: LabelSet::default(),
            };

            metric_value.increase();

            assert_eq!(metric_value.value, 1);
        }
    }

    mod a_label_pair_set {
        use std::collections::BTreeMap;

        use super::super::LabelSet;
        use crate::metrics::{LabelName, LabelValue};

        #[test]
        fn could_be_instantiated_from_a_b_tree_map() {
            let label_pair_set = LabelSet::new(BTreeMap::from([
                (LabelName::new("server_service_binding_protocol"), LabelValue::new("http")),
                (LabelName::new("server_service_binding_ip"), LabelValue::new("0.0.0.0")),
                (LabelName::new("server_service_binding_port"), LabelValue::new("7070")),
            ]));

            assert_eq!(
                label_pair_set
                    .set
                    .get(&LabelName::new("server_service_binding_protocol"))
                    .unwrap(),
                &LabelValue::new("http")
            );
            assert_eq!(
                label_pair_set.set.get(&LabelName::new("server_service_binding_ip")).unwrap(),
                &LabelValue::new("0.0.0.0")
            );
            assert_eq!(
                label_pair_set
                    .set
                    .get(&LabelName::new("server_service_binding_port"))
                    .unwrap(),
                &LabelValue::new("7070")
            );
        }

        #[test]
        fn should_allow_setting_a_new_label() {
            let mut label_pair_set = LabelSet::default();

            label_pair_set.set(LabelName::new("key"), LabelValue::new("value"));

            assert_eq!(
                label_pair_set.set.get(&LabelName::new("key")).unwrap(),
                &LabelValue::new("value")
            );
        }

        #[test]
        fn should_allow_updating_a_label_value() {
            let mut label_pair_set = LabelSet::default();

            label_pair_set.set(LabelName::new("key"), LabelValue::new("old value"));
            label_pair_set.set(LabelName::new("key"), LabelValue::new("new value"));

            assert_eq!(
                label_pair_set.set.get(&LabelName::new("key")).unwrap(),
                &LabelValue::new("new value")
            );
        }

        #[test]
        fn should_allow_serializing_to_json() {
            let label_pair_set = LabelSet::new(BTreeMap::from([(LabelName::new("key"), LabelValue::new("value"))]));

            let json = serde_json::to_string(&label_pair_set).unwrap();

            assert_eq!(
                formatjson::format_json(&json).unwrap(),
                formatjson::format_json(
                    r#"
                    {
                        "key":"value"
                    }
                    "#
                )
                .unwrap()
            );
        }

        #[test]
        fn should_serialize_to_json_with_label_names_alphabetically_ordered() {
            let label_pair_set = LabelSet::new(BTreeMap::from([
                (LabelName::new("a"), LabelValue::new("value a")),
                (LabelName::new("b"), LabelValue::new("value b")),
            ]));

            let json = serde_json::to_string(&label_pair_set).unwrap();

            assert_eq!(
                formatjson::format_json(&json).unwrap(),
                formatjson::format_json(
                    r#"
                    {
                        "a": "value a",
                        "b": "value b"
                    }
                    "#
                )
                .unwrap()
            );
        }
    }

    /* use std::collections::BTreeMap;

    use super::LabeledMetric;
    use crate::metrics::{LabelPairSet, LabeledMetrics, MetricMetadata, MetricValue};

    #[allow(clippy::no_effect_replace)]
    #[test]
    fn metrics_should_be_serializable_to_json() {
        let metrics = LabeledMetrics {
            metrics: vec![LabeledMetric {
                metadata: MetricMetadata {
                    name: "announce_requests_received_total".to_string(),
                    kind: "counter".to_string(),
                    values: vec![MetricValue {
                        value: 1,
                        update_at: "now".to_string(), // todo: use a real timestamp
                        labels: LabelPairSet {
                            set: metric_labels.clone(),
                        },
                    }],
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
                            "metadata": {
                                "name": "announce_requests_received_total",
                                "kind": "counter",
                                "value": 325
                            },
                            "values": [
                                {
                                    "value": 1,
                                    "update_at": "now",
                                    "labels": {
                                    "ip_version":"ipv4",
                                    "protocol":"udp",
                                    "url":"udp://127.0.0.1:6969"
                                }
                            ]
                        }
                    ]
                }"#
            )
            .unwrap()
        );
    }

    */
}
