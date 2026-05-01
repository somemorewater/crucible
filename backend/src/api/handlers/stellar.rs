use axum::{
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
};
use tracing::{info_span, info, instrument};

/// Handler for the /.well-known/stellar.toml (SEP-1) file.
/// This is essential for Stellar network identification and discovery.
#[instrument(skip_all, fields(http.method = "GET", http.route = "/.well-known/stellar.toml"))]
pub async fn get_stellar_toml() -> impl IntoResponse {
    let span = info_span!("stellar.toml.fetch");
    let _enter = span.enter();
    
    info!("Serving Stellar TOML for SEP-1 discovery");
    
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());
    // Critical: SEP-1 requires CORS * for wallet discovery
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());

    let toml_content = r#"
VERSION="2.0.0"
NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
ACCOUNTS=[
  "GBBD67IF6I2E7E5NCTZTTG46YAKMBH2O7662T7O4B5XW4YVRE3L363C6"
]
DOCUMENTATION_URL="https://github.com/your-org/crucible"

[[CURRENCIES]]
code="USDC"
issuer="GBBD67IF6I2E7E5NCTZTTG46YAKMBH2O7662T7O4B5XW4YVRE3L363C6"
display_decimals=6
"#;

    (StatusCode::OK, headers, toml_content)
}
