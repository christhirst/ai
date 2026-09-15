use crate::config::{default_admin_user, GrpcAuthConfig};
use base64::Engine;
use http::HeaderMap;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tonic::body::Body;
use tonic::codegen::http::Request;
use tonic::Status;
use tonic_middleware::{RequestInterceptor, RequestInterceptorLayer};

/// Represents the authenticated identity of a gRPC caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthIdentity {
    /// Basic Auth caller with admin role
    Admin(String),
    /// OAuth 2.0 Bearer token caller with subject and claims payload
    OAuth {
        sub: String,
        claims: serde_json::Value,
    },
}

#[derive(Debug)]
struct JwksCache {
    keys: RwLock<Option<(JwkSet, Instant)>>,
    ttl: Duration,
}

impl JwksCache {
    fn new(ttl: Duration) -> Self {
        Self {
            keys: RwLock::new(None),
            ttl,
        }
    }

    async fn get_keys(&self, client: &reqwest::Client, url: &str) -> Result<JwkSet, Status> {
        {
            let read_lock = self.keys.read().await;
            if let Some((ref jwks, fetched_at)) = *read_lock {
                if fetched_at.elapsed() < self.ttl {
                    return Ok(jwks.clone());
                }
            }
        }

        // Cache miss or expired: fetch fresh JWKS
        let mut write_lock = self.keys.write().await;
        // Double-check after acquiring write lock
        if let Some((ref jwks, fetched_at)) = *write_lock {
            if fetched_at.elapsed() < self.ttl {
                return Ok(jwks.clone());
            }
        }

        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| Status::internal(format!("Failed to fetch JWKS from {url}: {e}")))?;

        if !resp.status().is_success() {
            return Err(Status::internal(format!(
                "JWKS endpoint returned status {}: {url}",
                resp.status()
            )));
        }

        let jwks: JwkSet = resp
            .json()
            .await
            .map_err(|e| Status::internal(format!("Failed to parse JWKS JSON from {url}: {e}")))?;

        *write_lock = Some((jwks.clone(), Instant::now()));
        Ok(jwks)
    }
}

/// Core authentication validator for Basic Auth and OAuth 2.0.
#[derive(Debug)]
pub struct AuthValidator {
    pub admin_user: String,
    pub admin_password: Option<String>,
    pub jwks_url: Option<String>,
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub jwt_secret: Option<String>,
    pub jwt_public_key: Option<String>,
    pub static_tokens: Vec<String>,
    jwks_cache: JwksCache,
    http_client: reqwest::Client,
}

impl AuthValidator {
    pub fn new(config: &GrpcAuthConfig) -> Self {
        Self {
            admin_user: if config.admin_user.is_empty() {
                default_admin_user()
            } else {
                config.admin_user.clone()
            },
            admin_password: config.admin_password.clone(),
            jwks_url: config.oauth.jwks_url.clone(),
            issuer: config.oauth.issuer.clone(),
            audience: config.oauth.audience.clone(),
            jwt_secret: config.oauth.jwt_secret.clone(),
            jwt_public_key: config.oauth.jwt_public_key.clone(),
            static_tokens: config.oauth.static_tokens.clone(),
            jwks_cache: JwksCache::new(Duration::from_secs(300)),
            http_client: reqwest::Client::new(),
        }
    }

    /// Validates an authorization header value string.
    pub async fn validate_auth_header(&self, auth_header: Option<&str>) -> Result<AuthIdentity, Status> {
        let header = match auth_header {
            Some(h) if !h.trim().is_empty() => h.trim(),
            _ => return Err(Status::unauthenticated("Missing authorization metadata in request")),
        };

        if let Some(basic_part) = header.strip_prefix("Basic ").or_else(|| header.strip_prefix("basic ")) {
            return self.validate_basic_auth(basic_part.trim());
        }

        if let Some(bearer_part) = header.strip_prefix("Bearer ").or_else(|| header.strip_prefix("bearer ")) {
            return self.validate_bearer_token(bearer_part.trim()).await;
        }

        Err(Status::unauthenticated(
            "Unsupported authorization scheme. Expected 'Bearer <token>' or 'Basic <credentials>'",
        ))
    }

    /// Validates an incoming HTTP HeaderMap.
    pub async fn validate_headers(&self, headers: &HeaderMap) -> Result<AuthIdentity, Status> {
        let auth_str = match headers.get("authorization") {
            Some(v) => Some(
                v.to_str()
                    .map_err(|_| Status::unauthenticated("Invalid authorization header encoding"))?,
            ),
            None => None,
        };

        self.validate_auth_header(auth_str).await
    }

    /// Validates HTTP Basic Auth credentials for admin.
    fn validate_basic_auth(&self, encoded: &str) -> Result<AuthIdentity, Status> {
        let expected_password = match &self.admin_password {
            Some(p) if !p.is_empty() => p,
            _ => {
                return Err(Status::unauthenticated(
                    "Basic authentication is not configured on the server (admin password unset)",
                ));
            }
        };

        let decoded_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| Status::unauthenticated("Invalid Base64 in Basic authorization header"))?;

        let credentials = String::from_utf8(decoded_bytes)
            .map_err(|_| Status::unauthenticated("Invalid UTF-8 in decoded Basic credentials"))?;

        let (user, pass) = credentials
            .split_once(':')
            .ok_or_else(|| Status::unauthenticated("Malformed Basic credentials: expected user:password"))?;

        if user == self.admin_user && pass == expected_password {
            Ok(AuthIdentity::Admin(user.to_string()))
        } else {
            Err(Status::unauthenticated("Invalid basic auth credentials"))
        }
    }

    /// Validates an OAuth 2.0 Bearer token (JWT or static token).
    async fn validate_bearer_token(&self, token: &str) -> Result<AuthIdentity, Status> {
        // 1. Check static pre-shared tokens
        if self.static_tokens.iter().any(|t| t == token) {
            return Ok(AuthIdentity::OAuth {
                sub: "static_bearer_user".to_string(),
                claims: serde_json::json!({ "sub": "static_bearer_user", "role": "static_token" }),
            });
        }

        // 2. Parse JWT Header
        let header = decode_header(token)
            .map_err(|e| Status::unauthenticated(format!("Invalid JWT header in Bearer token: {e}")))?;

        // Prepare validation rules
        let mut validation = Validation::new(header.alg);
        if let Some(ref iss) = self.issuer {
            validation.set_issuer(&[iss]);
        } else {
            validation.validate_nbf = false;
        }

        if let Some(ref aud) = self.audience {
            validation.set_audience(&[aud]);
        } else {
            validation.validate_aud = false;
        }

        // 3. Validate signature based on configured key source and algorithm
        let is_asymmetric = matches!(
            header.alg,
            jsonwebtoken::Algorithm::RS256
                | jsonwebtoken::Algorithm::RS384
                | jsonwebtoken::Algorithm::RS512
                | jsonwebtoken::Algorithm::PS256
                | jsonwebtoken::Algorithm::PS384
                | jsonwebtoken::Algorithm::PS512
                | jsonwebtoken::Algorithm::ES256
                | jsonwebtoken::Algorithm::ES384
                | jsonwebtoken::Algorithm::EdDSA
        );

        let claims = if is_asymmetric {
            if let Some(ref jwks_url) = self.jwks_url {
                // JWKS Endpoint
                let jwks = self.jwks_cache.get_keys(&self.http_client, jwks_url).await?;
                let kid = header.kid.as_deref().ok_or_else(|| {
                    Status::unauthenticated("JWT token is missing 'kid' header required for JWKS verification")
                })?;

                let jwk = jwks
                    .find(kid)
                    .ok_or_else(|| Status::unauthenticated(format!("Key ID '{kid}' not found in JWKS from {jwks_url}")))?;

                let key = DecodingKey::from_jwk(jwk)
                    .map_err(|e| Status::internal(format!("Failed to construct DecodingKey from JWK '{kid}': {e}")))?;

                decode::<serde_json::Value>(token, &key, &validation)
                    .map_err(|e| Status::unauthenticated(format!("JWT validation failed (JWKS key '{kid}'): {e}")))?
                    .claims
            } else if let Some(ref pem_key) = self.jwt_public_key {
                // PEM Public Key (RSA or EC)
                let key = DecodingKey::from_rsa_pem(pem_key.as_bytes())
                    .or_else(|_| DecodingKey::from_ec_pem(pem_key.as_bytes()))
                    .or_else(|_| DecodingKey::from_ed_pem(pem_key.as_bytes()))
                    .map_err(|e| Status::internal(format!("Failed to parse configured JWT public key: {e}")))?;

                decode::<serde_json::Value>(token, &key, &validation)
                    .map_err(|e| Status::unauthenticated(format!("JWT validation failed (Public Key): {e}")))?
                    .claims
            } else {
                return Err(Status::unauthenticated(format!(
                    "OAuth Bearer token uses asymmetric algorithm '{:?}', but server has no configured JWKS URL or public key",
                    header.alg
                )));
            }
        } else if let Some(ref secret) = self.jwt_secret {
            // HMAC Shared Secret (HS256/384/512)
            let key = DecodingKey::from_secret(secret.as_bytes());
            decode::<serde_json::Value>(token, &key, &validation)
                .map_err(|e| Status::unauthenticated(format!("JWT validation failed (HMAC secret): {e}")))?
                .claims
        } else {
            return Err(Status::unauthenticated(
                "OAuth Bearer token presented, but server has no configured JWKS URL, JWT secret, or public key",
            ));
        };

        let sub = claims
            .get("sub")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown_sub")
            .to_string();

        Ok(AuthIdentity::OAuth { sub, claims })
    }
}

/// Request interceptor for tonic-middleware to authenticate all incoming gRPC calls.
#[derive(Clone)]
pub struct GrpcAuthInterceptor {
    pub validator: Arc<AuthValidator>,
}

impl GrpcAuthInterceptor {
    pub fn new(validator: Arc<AuthValidator>) -> Self {
        Self { validator }
    }
}

#[async_trait::async_trait]
impl RequestInterceptor for GrpcAuthInterceptor {
    async fn intercept(&self, mut req: Request<Body>) -> Result<Request<Body>, Status> {
        let identity = self.validator.validate_headers(req.headers()).await?;
        req.extensions_mut().insert(identity);
        Ok(req)
    }
}

/// Creates a tonic-middleware `RequestInterceptorLayer` configured from `GrpcAuthConfig`.
pub fn create_auth_layer(
    auth_config: &GrpcAuthConfig,
) -> Result<RequestInterceptorLayer<GrpcAuthInterceptor>, Box<dyn std::error::Error>> {
    let validator = Arc::new(AuthValidator::new(auth_config));
    let interceptor = GrpcAuthInterceptor::new(validator);
    Ok(RequestInterceptorLayer::new(interceptor))
}

/// OpenID Connect Discovery document parsed from `.well-known/openid-configuration`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OidcDiscoveryDocument {
    pub issuer: String,
    pub jwks_uri: String,
    #[serde(default)]
    pub token_endpoint: Option<String>,
}

/// Discovers OpenID Connect endpoints from a `.well-known/openid-configuration` URL.
pub async fn discover_oidc_endpoints(
    client: &reqwest::Client,
    well_known_url: &str,
) -> Result<OidcDiscoveryDocument, Box<dyn std::error::Error>> {
    let resp = client
        .get(well_known_url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to OIDC discovery URL '{well_known_url}': {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "OIDC discovery URL '{well_known_url}' returned HTTP status {}",
            resp.status()
        )
        .into());
    }

    let doc: OidcDiscoveryDocument = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse OIDC discovery JSON from '{well_known_url}': {e}"))?;

    Ok(doc)
}

/// Summary report returned when the startup OAuth check succeeds.
#[derive(Debug, Clone)]
pub struct OAuthCheckReport {
    pub issuer: Option<String>,
    pub jwks_url: Option<String>,
    pub token_url: Option<String>,
    pub client_id: Option<String>,
    pub verified_subject: Option<String>,
}

/// Performs startup health & authentication verification for OAuth 2.0 / OpenID Connect.
///
/// 1. If `well_known_url` is configured, automatically discovers `issuer`, `jwks_url`, and `token_url`.
/// 2. Verifies that the configured/discovered JWKS endpoint is reachable and returns valid signing keys.
/// 3. If client credentials (`client_id` + `client_secret`) are configured, tests acquiring a token
///    via client credentials grant and verifies that token with the server's AuthValidator.
pub async fn check_oauth_at_startup(
    auth_config: &mut GrpcAuthConfig,
) -> Result<Option<OAuthCheckReport>, Box<dyn std::error::Error>> {
    // If auth is explicitly disabled, or oauth is not configured, skip check
    if auth_config.enabled == Some(false) || !auth_config.oauth.is_configured() {
        return Ok(None);
    }

    // Check if startup check is explicitly disabled
    if auth_config.oauth.check_on_startup == Some(false) {
        return Ok(None);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;

    // 1. OIDC Discovery
    if let Some(ref wk_url) = auth_config.oauth.well_known_url {
        let doc = discover_oidc_endpoints(&client, wk_url).await?;
        if auth_config.oauth.issuer.is_none() {
            auth_config.oauth.issuer = Some(doc.issuer);
        }
        if auth_config.oauth.jwks_url.is_none() {
            auth_config.oauth.jwks_url = Some(doc.jwks_uri);
        }
        if auth_config.oauth.token_url.is_none() {
            auth_config.oauth.token_url = doc.token_endpoint;
        }
    }

    let validator = AuthValidator::new(auth_config);

    // 2. Validate PEM public key if configured
    if let Some(ref pem_key) = validator.jwt_public_key {
        DecodingKey::from_rsa_pem(pem_key.as_bytes())
            .or_else(|_| DecodingKey::from_ec_pem(pem_key.as_bytes()))
            .or_else(|_| DecodingKey::from_ed_pem(pem_key.as_bytes()))
            .map_err(|e| format!("Failed to parse configured JWT public key PEM: {e}"))?;
    }

    // 3. JWKS Check: warm cache and verify signing keys exist
    if let Some(ref jwks_url) = validator.jwks_url {
        let jwks = validator
            .jwks_cache
            .get_keys(&validator.http_client, jwks_url)
            .await
            .map_err(|status| {
                format!("Failed to retrieve JWKS signing keys from '{jwks_url}': {status}")
            })?;

        if jwks.keys.is_empty() {
            return Err(format!("JWKS endpoint '{jwks_url}' returned zero keys").into());
        }
    }

    // 4. Client Credentials handshake (if client_id + client_secret + token_url present)
    let mut verified_subject = None;
    if let (Some(client_id), Some(client_secret), Some(token_url)) = (
        &auth_config.oauth.client_id,
        &auth_config.oauth.client_secret,
        &auth_config.oauth.token_url,
    ) {
        let params = [
            ("grant_type", "client_credentials"),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
        ];

        let resp = client
            .post(token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Failed to connect to OAuth token endpoint '{token_url}': {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "OAuth token request failed at '{token_url}' with HTTP {status}: {body}"
            )
            .into());
        }

        let token_resp: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse token response JSON from '{token_url}': {e}"))?;

        let access_token = token_resp
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!("OAuth token endpoint response from '{token_url}' did not contain 'access_token'")
            })?;

        // Validate the acquired token using our AuthValidator
        let identity = validator
            .validate_auth_header(Some(&format!("Bearer {access_token}")))
            .await
            .map_err(|status| {
                format!("Failed to validate acquired OAuth token with server keys: {status}")
            })?;

        if let AuthIdentity::OAuth { sub, .. } = identity {
            verified_subject = Some(sub);
        }
    }

    Ok(Some(OAuthCheckReport {
        issuer: auth_config.oauth.issuer.clone(),
        jwks_url: auth_config.oauth.jwks_url.clone(),
        token_url: auth_config.oauth.token_url.clone(),
        client_id: auth_config.oauth.client_id.clone(),
        verified_subject,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    #[tokio::test]
    async fn test_basic_auth_valid() {
        let config = GrpcAuthConfig {
            enabled: Some(true),
            admin_user: "admin".to_string(),
            admin_password: Some("secret123".to_string()),
            oauth: Default::default(),
        };
        let validator = AuthValidator::new(&config);

        let encoded = base64::engine::general_purpose::STANDARD.encode("admin:secret123");
        let header = format!("Basic {encoded}");

        let identity = validator.validate_auth_header(Some(&header)).await.unwrap();
        assert_eq!(identity, AuthIdentity::Admin("admin".to_string()));
    }

    #[tokio::test]
    async fn test_basic_auth_invalid_password() {
        let config = GrpcAuthConfig {
            enabled: Some(true),
            admin_user: "admin".to_string(),
            admin_password: Some("secret123".to_string()),
            oauth: Default::default(),
        };
        let validator = AuthValidator::new(&config);

        let encoded = base64::engine::general_purpose::STANDARD.encode("admin:wrong_password");
        let header = format!("Basic {encoded}");

        let err = validator.validate_auth_header(Some(&header)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("Invalid basic auth credentials"));
    }

    #[tokio::test]
    async fn test_basic_auth_invalid_user() {
        let config = GrpcAuthConfig {
            enabled: Some(true),
            admin_user: "admin".to_string(),
            admin_password: Some("secret123".to_string()),
            oauth: Default::default(),
        };
        let validator = AuthValidator::new(&config);

        let encoded = base64::engine::general_purpose::STANDARD.encode("attacker:secret123");
        let header = format!("Basic {encoded}");

        let err = validator.validate_auth_header(Some(&header)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("Invalid basic auth credentials"));
    }

    #[tokio::test]
    async fn test_oauth_hmac_valid_jwt() {
        let secret = "super_secret_jwt_key_1234567890123456";
        let config = GrpcAuthConfig {
            enabled: Some(true),
            admin_user: "admin".to_string(),
            admin_password: None,
            oauth: crate::config::GrpcOauthConfig {
                jwt_secret: Some(secret.to_string()),
                issuer: Some("test-issuer".to_string()),
                audience: Some("test-aud".to_string()),
                ..Default::default()
            },
        };
        let validator = AuthValidator::new(&config);

        let exp = (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize;
        let claims = serde_json::json!({
            "sub": "user_42",
            "iss": "test-issuer",
            "aud": "test-aud",
            "exp": exp,
            "role": "editor"
        });

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let header = format!("Bearer {token}");
        let identity = validator.validate_auth_header(Some(&header)).await.unwrap();

        match identity {
            AuthIdentity::OAuth { sub, claims: c } => {
                assert_eq!(sub, "user_42");
                assert_eq!(c.get("role").and_then(|v| v.as_str()), Some("editor"));
            }
            _ => panic!("Expected OAuth identity"),
        }
    }

    #[tokio::test]
    async fn test_oauth_hmac_expired_jwt() {
        let secret = "super_secret_jwt_key_1234567890123456";
        let config = GrpcAuthConfig {
            enabled: Some(true),
            admin_user: "admin".to_string(),
            admin_password: None,
            oauth: crate::config::GrpcOauthConfig {
                jwt_secret: Some(secret.to_string()),
                ..Default::default()
            },
        };
        let validator = AuthValidator::new(&config);

        // Expired 1 hour ago
        let exp = (chrono::Utc::now() - chrono::Duration::hours(1)).timestamp() as usize;
        let claims = serde_json::json!({
            "sub": "user_42",
            "exp": exp
        });

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let header = format!("Bearer {token}");
        let err = validator.validate_auth_header(Some(&header)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("ExpiredSignature"));
    }

    #[tokio::test]
    async fn test_static_tokens() {
        let config = GrpcAuthConfig {
            enabled: Some(true),
            admin_user: "admin".to_string(),
            admin_password: None,
            oauth: crate::config::GrpcOauthConfig {
                static_tokens: vec!["static_dev_token_xyz".to_string()],
                ..Default::default()
            },
        };
        let validator = AuthValidator::new(&config);

        let header = "Bearer static_dev_token_xyz";
        let identity = validator.validate_auth_header(Some(header)).await.unwrap();
        match identity {
            AuthIdentity::OAuth { sub, .. } => assert_eq!(sub, "static_bearer_user"),
            _ => panic!("Expected OAuth identity"),
        }
    }

    #[tokio::test]
    async fn test_missing_auth_header() {
        let config = GrpcAuthConfig::default();
        let validator = AuthValidator::new(&config);

        let err = validator.validate_auth_header(None).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("Missing authorization metadata"));
    }

    #[tokio::test]
    async fn test_check_oauth_at_startup_disabled_returns_none() {
        let mut config = GrpcAuthConfig {
            enabled: Some(false),
            oauth: crate::config::GrpcOauthConfig {
                well_known_url: Some("http://127.0.0.1:9999/does-not-exist".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let report = check_oauth_at_startup(&mut config).await.unwrap();
        assert!(report.is_none());
    }

    #[tokio::test]
    async fn test_check_oauth_at_startup_check_on_startup_false_returns_none() {
        let mut config = GrpcAuthConfig {
            enabled: Some(true),
            oauth: crate::config::GrpcOauthConfig {
                well_known_url: Some("http://127.0.0.1:9999/does-not-exist".to_string()),
                check_on_startup: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };

        let report = check_oauth_at_startup(&mut config).await.unwrap();
        assert!(report.is_none());
    }

    #[tokio::test]
    async fn test_check_oauth_at_startup_invalid_pem_fails() {
        let mut config = GrpcAuthConfig {
            enabled: Some(true),
            oauth: crate::config::GrpcOauthConfig {
                jwt_public_key: Some("NOT_A_VALID_PEM_KEY".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let res = check_oauth_at_startup(&mut config).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("JWT public key PEM"));
    }

    #[tokio::test]
    async fn test_check_oauth_at_startup_static_tokens_succeeds() {
        let mut config = GrpcAuthConfig {
            enabled: Some(true),
            oauth: crate::config::GrpcOauthConfig {
                static_tokens: vec!["static_dev_token_123".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };

        let report = check_oauth_at_startup(&mut config).await.unwrap();
        assert!(report.is_some());
    }
}
