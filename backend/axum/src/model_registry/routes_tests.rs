#[cfg(test)]
mod tests {
    use crate::{firebase_auth::FirebaseVerifier, storage::SqliteStorage, AppState};
    use axum::{
        body::Body,
        http::{
            header::{AUTHORIZATION, CONTENT_TYPE},
            Request, StatusCode,
        },
    };
    use tower::ServiceExt;
    use uuid::Uuid;

    async fn app() -> axum::Router {
        let storage = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let state = AppState::with_storage(storage)
            .await
            .unwrap()
            .with_platform_admin_uids(vec!["platform-admin".into()])
            .with_firebase_verifier(FirebaseVerifier::test_verifier(), false);
        crate::app(state)
    }

    fn request(method: &str, path: &str, uid: Option<&str>, body: &str) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(uid) = uid {
            builder = builder.header(
                AUTHORIZATION,
                format!("Bearer {}", FirebaseVerifier::test_token(uid, false)),
            );
        }
        builder = builder.header(CONTENT_TYPE, "application/json");
        builder.body(Body::from(body.to_owned())).unwrap()
    }

    fn admin_routes() -> Vec<(&'static str, String, &'static str)> {
        let provider_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        vec![
            ("GET", "/api/v1/admin/providers".into(), "{}"),
            (
                "POST",
                "/api/v1/admin/providers".into(),
                r#"{"name":"Provider","base_url":"https://api.provider.test","status":"active"}"#,
            ),
            (
                "POST",
                ("/api/v1/admin/providers/".to_owned()
                    + &provider_id.to_string()
                    + "/terms-review"),
                r#"{"processing_regions":["eu"],"trains_on_api_inputs":false,"retention_days":0,"retention_description":"No retention"}"#,
            ),
            ("GET", "/api/v1/admin/models".into(), "{}"),
            (
                "POST",
                "/api/v1/admin/models".into(),
                r#"{"provider_id":"00000000-0000-0000-0000-000000000001","model_name":"candidate","abilities":["text"],"categories":["general"],"context_length":8000,"input_cost_per_million":1,"output_cost_per_million":1,"typical_latency_ms":1000,"regions":["eu"],"version":"v1","notes":"test"}"#,
            ),
            (
                "POST",
                ("/api/v1/admin/models/".to_owned() + &model_id.to_string() + "/review"),
                r#"{"action":"reject","note":"Test rejection"}"#,
            ),
            ("GET", "/api/v1/admin/evaluations".into(), "{}"),
            ("GET", "/api/v1/admin/stats".into(), "{}"),
            ("GET", "/api/v1/admin/flags".into(), "{}"),
            (
                "POST",
                ("/api/v1/admin/flags/".to_owned() + &model_id.to_string() + "/clear"),
                r#"{"note":"Reviewed and resolved"}"#,
            ),
            ("GET", "/api/v1/admin/assignments".into(), "{}"),
        ]
    }

    #[tokio::test]
    async fn every_admin_endpoint_rejects_an_authenticated_non_admin() {
        let app = app().await;
        for (method, path, body) in admin_routes() {
            let response = app
                .clone()
                .oneshot(request(method, &path, Some("ordinary-user"), body))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        }
    }

    #[tokio::test]
    async fn every_admin_endpoint_rejects_missing_and_expired_authentication() {
        let app = app().await;
        for (method, path, body) in admin_routes() {
            let missing = app
                .clone()
                .oneshot(request(method, &path, None, body))
                .await
                .unwrap();
            assert_eq!(
                missing.status(),
                StatusCode::UNAUTHORIZED,
                "missing: {method} {path}"
            );
            let expired = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(&path)
                        .header(
                            AUTHORIZATION,
                            format!(
                                "Bearer {}",
                                FirebaseVerifier::test_token("platform-admin", true)
                            ),
                        )
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from(body.to_owned()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                expired.status(),
                StatusCode::UNAUTHORIZED,
                "expired: {method} {path}"
            );
        }
    }

    #[tokio::test]
    async fn allowlisted_platform_admin_can_read_registry_but_workspace_owners_are_not_admins() {
        let app = app().await;
        let allowed = app
            .clone()
            .oneshot(request(
                "GET",
                "/api/v1/admin/models",
                Some("platform-admin"),
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        let owner = app
            .clone()
            .oneshot(request(
                "GET",
                "/api/v1/admin/models",
                Some("ordinary-user"),
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(owner.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn user_from_another_workspace_cannot_access_global_admin_endpoints() {
        let app = app().await;
        let response = app
            .oneshot(request(
                "GET",
                "/api/v1/admin/assignments",
                Some("unrelated-user"),
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
