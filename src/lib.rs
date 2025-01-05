use std::time::Duration;
use encoding_rs::{ISO_8859_10, UTF_8};
use std::borrow::Cow;
use std::str;
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use axum_extra::extract::Form;
use rss::Channel;

/// Name & Version of this application
pub const APP: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
pub const APP_REPO: &str = env!("CARGO_PKG_REPOSITORY");
/// Static user agent to avoid runtime allocations
pub const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);

pub type HttpClient = reqwest::Client;

/// Build a HTTP client
///
/// Client will be configured with optimized settings for RSS feed fetching:
/// - Shorter timeouts (RSS feeds should respond quickly)
/// - Larger connection pool (for parallel feed fetching)
/// - TCP keepalive (maintain connections for repeated requests)
/// - Static user agent (avoid allocations)
pub fn build_http_client() -> HttpClient {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(8) // Increased for better parallel performance
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .expect("build HTTP client")
}

/// Build the application router (sans state)
pub fn app() -> Router<HttpClient> {
    Router::new().route("/feed", get(feed))
}

/// Query parameters for the feed endpoint
#[derive(Debug, serde::Deserialize)]
pub struct FeedQuery {
    url: String,
    #[serde(default = "Default::default")]
    filter: Vec<String>,
}

/// GET /feed
pub async fn feed(
    State(http_client): State<reqwest::Client>,
    Form(query): Form<FeedQuery>,
) -> Result<Response, FeedError> {
    // Fetch upstream with streaming
    let req = http_client
        .get(query.url)
        .send()
        .await
        .map_err(FeedError::Fetch)?
        .error_for_status()
        .map_err(FeedError::Fetch)?;

    // Extract content type once, avoid allocations
    let is_iso_8859 = req
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.contains("ISO-8859-1"))
        .unwrap_or(false);
        

    // Read body with proper capacity pre-allocation
    let body = req.bytes().await.map_err(FeedError::Read)?;

    // Handle encoding more efficiently
    let (_, _, had_errors) = if is_iso_8859 {
        ISO_8859_10.decode(&body)
    } else {
        (
            Cow::Borrowed(std::str::from_utf8(&body).map_err(|_| FeedError::Encoding)?),
            UTF_8,
            false,
        )
    };
    
    if had_errors {
        return Err(FeedError::Encoding);
    }
    
    let mut channel = Channel::read_from(&body[..]).map_err(FeedError::Parse)?;

    // Filter items in-place to avoid allocation
    if !query.filter.is_empty() {
        channel.items.retain(|item| {
            !item
                .title
                .as_ref()
                .map(|title| query.filter.iter().any(|fp| title.contains(fp)))
                .unwrap_or(false)
        });
    }

    // Render back as RSS
    Ok((
        [
            (header::SERVER, APP),
            (header::CONTENT_TYPE, "application/rss+xml; charset=UTF-8"),
        ],
        channel.to_string(),
    )
        .into_response())
}

/// Errors that might occur on the feed endpoint
#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("Failed to fetch upstream feed: {0}")]
    Fetch(reqwest::Error),

    #[error("Failed to read upstream body: {0}")]
    Read(reqwest::Error),

    #[error("Failed to parse upstream body: {0}")]
    Parse(rss::Error),

    #[error("Failed to encode content to UTF-8")]
    Encoding,
}

impl IntoResponse for FeedError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_GATEWAY, self).into_response()
    }
}