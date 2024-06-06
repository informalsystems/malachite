use axum::routing::get;
use axum::Router;
use malachite_node::config::MetricsConfig;
use tokio::net::TcpListener;
use tracing::info;

#[tracing::instrument(name = "metrics", skip_all)]
pub async fn serve_metrics(config: MetricsConfig) {
    let app = Router::new().route("/metrics", get(get_metrics));
    let listener = TcpListener::bind(config.listen_addr).await.unwrap();

    info!("Serving metrics at http://{}", config.listen_addr);
    axum::serve(listener, app).await.unwrap();
}

async fn get_metrics() -> String {
    let mut buf = String::new();
    malachite_metrics::export(&mut buf);
    buf
}
