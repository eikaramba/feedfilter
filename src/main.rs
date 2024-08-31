use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() {
    // determine listen address
    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "[::1]:4159".to_string());

    // bind to port
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("bind to {addr}: {err}"));

    // build HTTP client
    let http_client = feedfilter::build_http_client();

    // build application router
    let app = feedfilter::app().with_state(http_client);

    // start up
    println!("---> Listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

/// This method will block (async) until either SIGTERM or SIGINT have been sent to the process.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler")
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
