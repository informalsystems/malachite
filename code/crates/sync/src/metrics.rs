use std::ops::Deref;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use malachitebft_metrics::SharedRegistry;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::{exponential_buckets, Histogram};

pub type DecidedValuesMetrics = Inner;

#[derive(Clone, Debug)]
pub struct Metrics(Arc<DecidedValuesMetrics>);

impl Deref for Metrics {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct RequestLabels {
    batch_size: u64,
}

#[derive(Debug)]
pub struct Inner {
    requests_sent: Family<RequestLabels, Counter>,
    requests_received: Family<RequestLabels, Counter>,
    responses_sent: Family<RequestLabels, Counter>,
    responses_received: Family<RequestLabels, Counter>,
    client_latency: Histogram,
    server_latency: Histogram,
    request_timeouts: Counter,

    instant_request_sent: Arc<DashMap<(u64, i64), Instant>>,
    instant_request_received: Arc<DashMap<(u64, i64), Instant>>,
}

impl Inner {
    pub fn new() -> Self {
        Self {
            requests_sent: Family::default(),
            requests_received: Family::default(),
            responses_sent: Family::default(),
            responses_received: Family::default(),
            client_latency: Histogram::new(exponential_buckets(0.1, 2.0, 20)),
            server_latency: Histogram::new(exponential_buckets(0.1, 2.0, 20)),
            request_timeouts: Counter::default(),
            instant_request_sent: Arc::new(DashMap::new()),
            instant_request_received: Arc::new(DashMap::new()),
        }
    }
}

impl Default for Inner {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self(Arc::new(DecidedValuesMetrics::new()))
    }

    fn decided_values(&self) -> &DecidedValuesMetrics {
        &self.0
    }

    pub fn register(registry: &SharedRegistry) -> Self {
        let metrics = Self::new();

        registry.with_prefix("malachitebft_sync", |registry| {
            // Value sync related metrics
            registry.register(
                "value_requests_sent",
                "Number of ValueSync requests sent",
                metrics.decided_values().requests_sent.clone(),
            );

            registry.register(
                "value_requests_received",
                "Number of ValueSync requests received",
                metrics.decided_values().requests_received.clone(),
            );

            registry.register(
                "value_responses_sent",
                "Number of ValueSync responses sent",
                metrics.decided_values().responses_sent.clone(),
            );

            registry.register(
                "value_responses_received",
                "Number of ValueSync responses received",
                metrics.decided_values().responses_received.clone(),
            );

            registry.register(
                "value_client_latency",
                "Interval of time between when request was sent and response was received",
                metrics.decided_values().client_latency.clone(),
            );

            registry.register(
                "value_server_latency",
                "Interval of time between when request was received and response was sent",
                metrics.decided_values().server_latency.clone(),
            );

            registry.register(
                "value_request_timeouts",
                "Number of ValueSync request timeouts",
                metrics.decided_values().request_timeouts.clone(),
            );
        });

        metrics
    }

    pub fn decided_value_request_sent(&self, height: u64, batch_size: u64) {
        self.decided_values()
            .requests_sent
            .get_or_create(&RequestLabels { batch_size })
            .inc();
        self.decided_values()
            .instant_request_sent
            .insert((height, -1), Instant::now());
    }

    pub fn decided_value_request_received(&self, height: u64, batch_size: u64) {
        self.decided_values()
            .requests_received
            .get_or_create(&RequestLabels { batch_size })
            .inc();
        self.decided_values()
            .instant_request_received
            .insert((height, -1), Instant::now());
    }

    pub fn decided_value_response_sent(&self, height: u64, batch_size: u64) {
        self.decided_values()
            .responses_sent
            .get_or_create(&RequestLabels { batch_size })
            .inc();

        if let Some((_, instant)) = self
            .decided_values()
            .instant_request_received
            .remove(&(height, -1))
        {
            self.decided_values()
                .server_latency
                .observe(instant.elapsed().as_secs_f64());
        }
    }

    pub fn decided_value_response_received(&self, height: u64, batch_size: u64) {
        self.decided_values()
            .responses_received
            .get_or_create(&RequestLabels { batch_size })
            .inc();

        if let Some((_, instant)) = self
            .decided_values()
            .instant_request_sent
            .remove(&(height, -1))
        {
            self.decided_values()
                .client_latency
                .observe(instant.elapsed().as_secs_f64());
        }
    }

    pub fn decided_value_request_timed_out(&self, height: u64) {
        self.decided_values().request_timeouts.inc();
        // TODO(SYNC): Check if this is correct: key (height, 0) is never inserted
        self.decided_values()
            .instant_request_sent
            .remove(&(height, 0));
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
