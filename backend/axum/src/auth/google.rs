use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::{sync::Arc, time::{Duration, Instant}};
use tokio::sync::RwLock;

const GOOGLE_JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const CLOCK_SKEW_SECONDS: u64 = 60;

#[derive(Debug, Clone, Deserialize)]
pub struct GoogleClaims {
    pub iss: String, pub aud: String, pub sub: String, pub exp: u64, pub iat: u64,
    pub email: String, pub email_verified: bool, pub name: Option<String>, pub picture: Option<String>,
}
#[derive(Debug, Clone)]
pub struct GoogleIdentity { pub sub: String, pub email: String, pub name: Option<String>, pub picture: Option<String> }
#[derive(Debug, thiserror::Error)]
pub enum GoogleAuthError {
    #[error("missing or malformed bearer token")] MissingToken,
    #[error("Google OAuth client IDs are not configured")] MissingClientIds,
    #[error("Google signing keys are unavailable")] KeyService,
    #[error("token header is invalid")] InvalidHeader,
    #[error("token signing key is unknown")] UnknownKey,
    #[error("Google ID token signature or claims are invalid")] InvalidToken,
}
#[derive(Debug, Deserialize)]
struct JwksDocument { keys: Vec<Jwk> }
#[derive(Debug, Clone, Deserialize)]
struct Jwk { kid: String, kty: String, alg: Option<String>, n: String, e: String }
struct CachedJwks { keys: Vec<Jwk>, expires_at: Instant, refreshed_at: Instant }

#[derive(Clone)]
pub struct GoogleVerifier {
    client_ids: Arc<[String]>,
    client: reqwest::Client,
    cache: Arc<RwLock<Option<CachedJwks>>>,
    #[cfg(test)] test_mode: bool,
}
impl GoogleVerifier {
    pub fn new(client_ids: Vec<String>) -> Result<Self, GoogleAuthError> {
        let ids: Vec<String> = client_ids.into_iter().map(|id| id.trim().to_owned()).filter(|id| !id.is_empty()).collect();
        if ids.is_empty() { return Err(GoogleAuthError::MissingClientIds); }
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).user_agent("axon-api/google-oauth").build().map_err(|_| GoogleAuthError::KeyService)?;
        Ok(Self { client_ids: Arc::from(ids), client, cache: Arc::new(RwLock::new(None)), #[cfg(test)] test_mode: false })
    }
    async fn keys(&self, force: bool) -> Result<Vec<Jwk>, GoogleAuthError> {
        if let Some(cached) = self.cache.read().await.as_ref() {
            let now = Instant::now();
            if !force && cached.expires_at > now { return Ok(cached.keys.clone()); }
            if force && now.duration_since(cached.refreshed_at) < Duration::from_secs(5) { return Ok(cached.keys.clone()); }
        }
        let response = self.client.get(GOOGLE_JWKS_URL).send().await.map_err(|_| GoogleAuthError::KeyService)?;
        if !response.status().is_success() { return Err(GoogleAuthError::KeyService); }
        let ttl = response.headers().get(reqwest::header::CACHE_CONTROL).and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').find_map(|p| p.trim().strip_prefix("max-age=")))
            .and_then(|v| v.parse::<u64>().ok()).unwrap_or(3600).clamp(60, 86_400);
        let doc = response.json::<JwksDocument>().await.map_err(|_| GoogleAuthError::KeyService)?;
        let keys: Vec<Jwk> = doc.keys.into_iter().filter(|k| k.kty == "RSA" && k.alg.as_deref().unwrap_or("RS256") == "RS256").collect();
        if keys.is_empty() { return Err(GoogleAuthError::KeyService); }
        *self.cache.write().await = Some(CachedJwks { keys: keys.clone(), expires_at: Instant::now()+Duration::from_secs(ttl), refreshed_at: Instant::now() });
        Ok(keys)
    }
    #[cfg(test)]
    pub(crate) fn test_verifier() -> Self {
        Self { client_ids: Arc::from(vec!["axon-test-client.apps.googleusercontent.com".to_owned()]), client: reqwest::Client::new(), cache: Arc::new(RwLock::new(None)), test_mode: true }
    }
    #[cfg(test)]
    pub(crate) fn test_token(sub: &str, expired: bool) -> String { format!("axon-google-test:{sub}:{}", if expired {"expired"} else {"valid"}) }
    pub async fn verify(&self, token: &str) -> Result<GoogleIdentity, GoogleAuthError> {
        if token.is_empty() || token.len() > 16_384 { return Err(GoogleAuthError::MissingToken); }
        #[cfg(test)]
        if self.test_mode {
            if let Some(v) = token.strip_prefix("axon-google-test:") {
                let (sub, exp) = v.rsplit_once(':').ok_or(GoogleAuthError::InvalidToken)?;
                if exp != "valid" || sub.is_empty() { return Err(GoogleAuthError::InvalidToken); }
                return Ok(GoogleIdentity { sub: sub.into(), email: format!("{sub}@example.com"), name: Some("Axon Test User".into()), picture: None });
            }
        }
        let header = decode_header(token).map_err(|_| GoogleAuthError::InvalidHeader)?;
        if header.alg != Algorithm::RS256 { return Err(GoogleAuthError::InvalidHeader); }
        let kid = header.kid.ok_or(GoogleAuthError::InvalidHeader)?;
        let mut keys = self.keys(false).await?;
        let mut key = keys.iter().find(|k| k.kid == kid).cloned();
        if key.is_none() { keys = self.keys(true).await?; key = keys.iter().find(|k| k.kid == kid).cloned(); }
        let key = key.ok_or(GoogleAuthError::UnknownKey)?;
        let decoding_key = DecodingKey::from_rsa_components(&key.n, &key.e).map_err(|_| GoogleAuthError::InvalidToken)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&self.client_ids.iter().map(String::as_str).collect::<Vec<_>>());
        validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);
        validation.leeway = CLOCK_SKEW_SECONDS;
        validation.validate_exp = true;
        validation.validate_nbf = true;
        let claims = decode::<GoogleClaims>(token, &decoding_key, &validation).map_err(|_| GoogleAuthError::InvalidToken)?.claims;
        if claims.sub.is_empty() || claims.sub.len() > 255 || claims.email.trim().is_empty() || !claims.email_verified {
            return Err(GoogleAuthError::InvalidToken);
        }
        Ok(GoogleIdentity { sub: claims.sub, email: claims.email, name: claims.name, picture: claims.picture })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verifier_requires_oauth_client_ids() {
        assert!(matches!(GoogleVerifier::new(vec![]), Err(GoogleAuthError::MissingClientIds)));
        assert!(matches!(GoogleVerifier::new(vec![" ".into()]), Err(GoogleAuthError::MissingClientIds)));
    }
    #[tokio::test]
    async fn test_verifier_accepts_identity_and_rejects_expired_token() {
        let v = GoogleVerifier::test_verifier();
        let id = v.verify(&GoogleVerifier::test_token("google-sub", false)).await.unwrap();
        assert_eq!(id.sub, "google-sub");
        assert_eq!(id.email, "google-sub@example.com");
        assert!(matches!(v.verify(&GoogleVerifier::test_token("google-sub", true)).await, Err(GoogleAuthError::InvalidToken)));
    }
}
