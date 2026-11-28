#[cfg(test)]
mod tests {
    use crate::{
        firebase_auth::FirebaseVerifier,
        memory_store::MemoryStore,
        model::{Agent, ConnectionMode, Task, TaskStatus},
        storage::SqliteStorage,
        AppState,
    };
    use axum::{
        body::{to_bytes, Body},
        http::{
            header::{AUTHORIZATION, CONTENT_TYPE},
            Request, StatusCode,
        },
    };
    use chrono::Utc;
    use tower::ServiceExt;
    use uuid::Uuid;

    async fn test_app() -> (axum::Router, AppState, Uuid) {
        let sqlite = SqliteStorage::connect_in_memory().await.unwrap();
        let memory_store = MemoryStore::new(sqlite.sqlite_pool());
        let storage = sqlite.shared();
        let state = AppState::with_storage(storage)
            .await
            .unwrap()
            .with_agent_memory_store(Some(memory_store))
            .with_firebase_verifier(FirebaseVerifier::test_verifier(), true);
        let first = state
            .ensure_firebase_user("axon-test-project", "owner-user")
            .await
            .unwrap();
        state
            .workspaces
            .write()
            .await
            .insert(first.workspace.id, first.workspace.clone());
        let task_id = Uuid::new_v4();
        state.tasks.write().await.insert(
            task_id,
            Task {
                id: task_id,
                workspace_id: first.workspace.id,
                title: "Private task".into(),
                objective: "Test resource isolation".into(),
                assignee_agent_id: Uuid::new_v4(),
                context_artifact_ids: Vec::new(),
                response_artifact_id: None,
                status: TaskStatus::Offered,
                version: 1,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                transitions: Vec::new(),
            },
        );
        (crate::app(state.clone()), state, task_id)
    }

    fn json_request(
        method: &str,
        path: &str,
        token: Option<String>,
        body: serde_json::Value,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    fn request(method: &str, path: &str, token: Option<String>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn every_registered_api_route_rejects_missing_and_expired_tokens() {
        let (app, state, task_id) = test_app().await;
        let workspace_id = state
            .ensure_firebase_user("axon-test-project", "owner-user")
            .await
            .unwrap()
            .workspace
            .id;
        let routes: Vec<(&str, String)> = vec![
            ("GET", "/api/v1/workspaces".into()),
            ("POST", "/api/v1/workspaces".into()),
            ("GET", format!("/api/v1/workspaces/{workspace_id}/audit")),
            ("GET", "/api/v1/sim/devices/simulator-1/telemetry".into()),
            ("POST", "/api/v1/sim/devices/simulator-1/telemetry".into()),
            ("GET", format!("/api/v1/workspaces/{workspace_id}/agents")),
            ("GET", format!("/api/v1/workspaces/{workspace_id}/chats")),
            ("POST", format!("/api/v1/workspaces/{workspace_id}/chats")),
            (
                "GET",
                format!("/api/v1/workspaces/{workspace_id}/chats/{}", Uuid::new_v4()),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/read",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/like",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/messages",
                    Uuid::new_v4()
                ),
            ),
            (
                "PATCH",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/archive",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/memory",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/memory/{}/approve",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            ("GET", format!("/api/v1/explore/agents/{}", Uuid::new_v4())),
            ("POST", format!("/api/v1/workspaces/{workspace_id}/agents")),
            (
                "GET",
                format!("/api/v1/workspaces/{workspace_id}/artifacts"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/{workspace_id}/artifacts"),
            ),
            ("GET", format!("/api/v1/workspaces/{workspace_id}/tasks")),
            ("POST", format!("/api/v1/workspaces/{workspace_id}/tasks")),
            ("POST", format!("/api/v1/workspaces/{workspace_id}/capability-polls")),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/{workspace_id}/capability-polls/{}",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/capability-polls/{}/nominations",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/capability-polls/{}/finalize",
                    Uuid::new_v4()
                ),
            ),
            ("GET", format!("/api/v1/workspaces/{workspace_id}/supervisor-runs")),
            ("POST", format!("/api/v1/workspaces/{workspace_id}/supervisor-runs")),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/{workspace_id}/supervisor-runs/{}",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/supervisor-runs/{}/checklists",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/supervisor-runs/{}/workers",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/supervisor-runs/{}/checks",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/supervisor-runs/{}/reviews",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/supervisor-runs/{}/repair",
                    Uuid::new_v4()
                ),
            ),
            ("POST", format!("/api/v1/tasks/{task_id}/submit")),
            ("POST", format!("/api/v1/tasks/{task_id}/execute")),
            ("POST", format!("/api/v1/tasks/{task_id}/transition")),
            (
                "POST",
                format!("/api/v1/workspaces/{workspace_id}/agent-memories"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/{workspace_id}/agent-memories/allocate"),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agent-memories/{}/review",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agent-memories/{}/share",
                    Uuid::new_v4()
                ),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agent-memories/{}/share/{}",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            ("GET", "/api/v1/admin/providers".into()),
            ("POST", "/api/v1/admin/providers".into()),
            ("POST", "/api/v1/admin/providers/provider-1/terms-review".into()),
            ("GET", "/api/v1/admin/models".into()),
            ("POST", "/api/v1/admin/models".into()),
            ("POST", "/api/v1/admin/models/model-1/review".into()),
            ("GET", "/api/v1/admin/evaluations".into()),
            ("GET", "/api/v1/admin/stats".into()),
            ("GET", "/api/v1/admin/flags".into()),
            ("POST", "/api/v1/admin/flags/flag-1/clear".into()),
            ("GET", "/api/v1/admin/assignments".into()),
            ("GET", "/api/v1/explore/agents".into()),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/publish",
                    Uuid::new_v4()
                ),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/publish",
                    Uuid::new_v4()
                ),
            ),
            (
                "GET",
                format!("/api/v1/explore/agents/{}/comments", Uuid::new_v4()),
            ),
            (
                "POST",
                format!("/api/v1/explore/agents/{}/comments", Uuid::new_v4()),
            ),
            (
                "PATCH",
                format!("/api/v1/explore/comments/{}", Uuid::new_v4()),
            ),
            (
                "DELETE",
                format!("/api/v1/explore/comments/{}", Uuid::new_v4()),
            ),
            (
                "POST",
                format!("/api/v1/explore/agents/{}/like", Uuid::new_v4()),
            ),
            (
                "POST",
                format!("/api/v1/explore/agents/{}/follow", Uuid::new_v4()),
            ),
            (
                "POST",
                format!("/api/v1/explore/comments/{}/like", Uuid::new_v4()),
            ),
            ("POST", "/api/v1/explore/reports".into()),
            (
                "GET",
                format!("/api/v1/explore/agents/{}/reports", Uuid::new_v4()),
            ),
            (
                "POST",
                format!("/api/v1/explore/reports/{}/resolve", Uuid::new_v4()),
            ),
            ("GET", "/api/v1/notifications".into()),
            ("POST", "/api/v1/notifications/read-all".into()),
            (
                "POST",
                format!("/api/v1/notifications/{}/read", Uuid::new_v4()),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/{workspace_id}/memory/documents"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/{workspace_id}/memory/reconciliation"),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/memory/documents/{}/1/approve",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/memory/documents/{}/1/archive",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/memory/documents/{}/1/restore",
                    Uuid::new_v4()
                ),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/{workspace_id}/memory/documents/{}/1",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/memory/documents/{}/1/retry-sync",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/credentials",
                    Uuid::new_v4()
                ),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/credentials/{}",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/credentials/{}/rotate",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/memory-documents/{}/1",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/memory-documents/{}/versions/history",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/memory-documents/search",
                    Uuid::new_v4()
                ),
            ),
            (
                "GET",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/memory-sessions/{}/documents",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/{workspace_id}/memory/index"),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/messages/{}/memory-index",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/{workspace_id}/memory/epoch"),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/{workspace_id}/memory/qoin-vector"),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/memory-index/{}/qoin",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/chats/{}/messages/{}/qoin-index",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/memory-context",
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/memory-sessions/{}/retrieve",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/memory-sessions/{}/context",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/agents/{}/memory-sessions/{}/feedback",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/{workspace_id}/members"),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/{workspace_id}/members/{}",
                    Uuid::new_v4()
                ),
            ),
            (
                "GET",
                format!("/api/v1/workspaces/{workspace_id}/projects"),
            ),
            (
                "POST",
                format!("/api/v1/workspaces/{workspace_id}/projects"),
            ),
            (
                "POST",
                format!(
                    "/api/v1/workspaces/{workspace_id}/projects/{}/members",
                    Uuid::new_v4()
                ),
            ),
            (
                "DELETE",
                format!(
                    "/api/v1/workspaces/{workspace_id}/projects/{}/members/{}",
                    Uuid::new_v4(),
                    Uuid::new_v4()
                ),
            ),
        ];
        for (method, path) in routes {
            let missing = app
                .clone()
                .oneshot(request(method, &path, None))
                .await
                .unwrap();
            assert_eq!(
                missing.status(),
                StatusCode::UNAUTHORIZED,
                "missing token: {method} {path}"
            );
            let expired = app
                .clone()
                .oneshot(request(
                    method,
                    &path,
                    Some(FirebaseVerifier::test_token("expired-user", true)),
                ))
                .await
                .unwrap();
            assert_eq!(
                expired.status(),
                StatusCode::UNAUTHORIZED,
                "expired token: {method} {path}"
            );
        }
    }

    #[tokio::test]
    async fn another_users_workspace_and_task_resources_are_not_found() {
        let (app, state, task_id) = test_app().await;
        let workspace_id = state
            .ensure_firebase_user("axon-test-project", "owner-user")
            .await
            .unwrap()
            .workspace
            .id;
        let owner = state
            .ensure_firebase_user("axon-test-project", "owner-user")
            .await
            .unwrap();
        let chat = state
            .chat_memory
            .as_ref()
            .unwrap()
            .create_chat(
                workspace_id,
                owner.user_id,
                &format!("human:{}", owner.user_id),
                "Private chat".into(),
            )
            .await
            .unwrap();
        let other_user_token = FirebaseVerifier::test_token("other-user", false);
        let paths = [
            format!("/api/v1/workspaces/{workspace_id}/audit"),
            format!("/api/v1/workspaces/{workspace_id}/chats"),
            format!("/api/v1/workspaces/{workspace_id}/chats/{}", chat.id),
            format!(
                "/api/v1/workspaces/{workspace_id}/chats/{}/messages",
                chat.id
            ),
            format!(
                "/api/v1/workspaces/{workspace_id}/chats/{}/archive",
                chat.id
            ),
            format!("/api/v1/workspaces/{workspace_id}/chats/{}/memory", chat.id),
            format!("/api/v1/workspaces/{workspace_id}/agents"),
            format!("/api/v1/workspaces/{workspace_id}/artifacts"),
            format!("/api/v1/workspaces/{workspace_id}/tasks"),
            format!("/api/v1/workspaces/{workspace_id}/capability-polls"),
            format!("/api/v1/workspaces/{workspace_id}/supervisor-runs"),
            format!("/api/v1/workspaces/{workspace_id}/memory/documents"),
            format!("/api/v1/workspaces/{workspace_id}/projects"),
            format!("/api/v1/workspaces/{workspace_id}/members"),
            format!(
                "/api/v1/workspaces/{workspace_id}/agents/{}/credentials",
                Uuid::new_v4()
            ),
            format!("/api/v1/tasks/{task_id}/submit"),
            format!("/api/v1/tasks/{task_id}/transitions"),
        ];
        for path in paths {
            let method = if path.contains("/submit")
                || path.contains("/transitions")
                || path.contains("/capability-polls")
                || path.contains("/supervisor-runs")
                || path.contains("/messages")
                || path.contains("/memory")
                || path.contains("/agent-memories")
                || path.ends_with("/members")
                || path.contains("/credentials")
                || path.contains("/projects/")
            {
                "POST"
            } else if path.ends_with("/archive") {
                "PATCH"
            } else {
                "GET"
            };
            let response = app
                .clone()
                .oneshot(request(method, &path, Some(other_user_token.clone())))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "cross-user access: {method} {path}"
            );
        }
    }

    #[tokio::test]
    async fn health_and_readiness_are_public_routes() {
        let (app, _, _) = test_app().await;
        let health = app
            .clone()
            .oneshot(request("GET", "/health", None))
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let readiness = app
            .clone()
            .oneshot(request("GET", "/ready", None))
            .await
            .unwrap();
        assert_eq!(readiness.status(), StatusCode::OK);
        let api = app
            .oneshot(request("GET", "/api/v1/workspaces", None))
            .await
            .unwrap();
        assert_eq!(api.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn simulator_routes_are_hidden_when_development_flag_is_disabled() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let state = AppState::with_storage(storage)
            .await
            .unwrap()
            .with_firebase_verifier(FirebaseVerifier::test_verifier(), false);
        let app = crate::app(state);
        let token = FirebaseVerifier::test_token("dev-user", false);
        let response = app
            .oneshot(request(
                "GET",
                "/api/v1/sim/devices/simulator-1/telemetry",
                Some(token),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    #[tokio::test]
    async fn scoped_agent_memory_requires_review_and_explicit_cross_agent_grants() {
        let (app, state, _) = test_app().await;
        let owner = state
            .ensure_firebase_user("axon-test-project", "owner-user")
            .await
            .unwrap();
        let owner_agent_id = Uuid::new_v4();
        let recipient_agent_id = Uuid::new_v4();
        for (id, name) in [
            (owner_agent_id, "Owner agent"),
            (recipient_agent_id, "Recipient agent"),
        ] {
            state.agents.write().await.insert(
                id,
                Agent {
                    id,
                    workspace_id: owner.workspace.id,
                    created_by_user_id: Some(owner.user_id),
                    display_name: name.into(),
                    provider: "test".into(),
                    connection_mode: ConnectionMode::Manual,
                    created_at: Utc::now(),
                },
            );
        }
        let token = FirebaseVerifier::test_token("owner-user", false);
        let base = format!("/api/v1/workspaces/{}/agent-memories", owner.workspace.id);

        let proposed = app
            .clone()
            .oneshot(json_request(
                "POST",
                &base,
                Some(token.clone()),
                serde_json::json!({
                    "scope": "agent",
                    "scope_id": null,
                    "owner_agent_id": owner_agent_id,
                    "content": "The worker uses an evidence first deterministic review process.",
                    "source": "reviewed supervisor note",
                    "sensitivity": "internal",
                    "expires_at": null
                }),
            ))
            .await
            .unwrap();
        assert_eq!(proposed.status(), StatusCode::CREATED);
        let proposal_body = to_bytes(proposed.into_body(), usize::MAX).await.unwrap();
        let proposal: serde_json::Value = serde_json::from_slice(&proposal_body).unwrap();
        let memory_id = Uuid::parse_str(proposal["memory_id"].as_str().unwrap()).unwrap();

        let owner_allocation = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("{base}/allocate"),
                Some(token.clone()),
                serde_json::json!({
                    "task_id": null,
                    "agent_id": owner_agent_id,
                    "token_budget": 500,
                    "query": "evidence first deterministic review process"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(owner_allocation.status(), StatusCode::OK);
        let body = to_bytes(owner_allocation.into_body(), usize::MAX)
            .await
            .unwrap();
        let allocation: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(allocation["selected"].as_array().unwrap().len(), 1);

        let reviewed = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("{base}/{memory_id}/review"),
                Some(token.clone()),
                serde_json::json!({"decision": "approve"}),
            ))
            .await
            .unwrap();
        assert_eq!(reviewed.status(), StatusCode::OK);

        let owner_allocation = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("{base}/allocate"),
                Some(token.clone()),
                serde_json::json!({
                    "task_id": null,
                    "agent_id": owner_agent_id,
                    "token_budget": 500,
                    "query": "evidence first deterministic review process"
                }),
            ))
            .await
            .unwrap();
        let body = to_bytes(owner_allocation.into_body(), usize::MAX)
            .await
            .unwrap();
        let allocation: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(allocation["selected"].as_array().unwrap().len(), 1);

        let recipient_allocation = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("{base}/allocate"),
                Some(token.clone()),
                serde_json::json!({
                    "task_id": null,
                    "agent_id": recipient_agent_id,
                    "token_budget": 500,
                    "query": "evidence first deterministic review process"
                }),
            ))
            .await
            .unwrap();
        let body = to_bytes(recipient_allocation.into_body(), usize::MAX)
            .await
            .unwrap();
        let allocation: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(allocation["selected"].as_array().unwrap().is_empty());

        let shared = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("{base}/{memory_id}/share"),
                Some(token.clone()),
                serde_json::json!({"agent_id": recipient_agent_id}),
            ))
            .await
            .unwrap();
        assert_eq!(shared.status(), StatusCode::NO_CONTENT);

        let recipient_allocation = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("{base}/allocate"),
                Some(token.clone()),
                serde_json::json!({
                    "task_id": null,
                    "agent_id": recipient_agent_id,
                    "token_budget": 500,
                    "query": "evidence first deterministic review process"
                }),
            ))
            .await
            .unwrap();
        let body = to_bytes(recipient_allocation.into_body(), usize::MAX)
            .await
            .unwrap();
        let allocation: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(allocation["selected"].as_array().unwrap().len(), 1);

        let revoked = app
            .clone()
            .oneshot(request(
                "DELETE",
                &format!("{base}/{memory_id}/share/{recipient_agent_id}"),
                Some(token.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::NO_CONTENT);

        let recipient_allocation = app
            .oneshot(json_request(
                "POST",
                &format!("{base}/allocate"),
                Some(token),
                serde_json::json!({
                    "task_id": null,
                    "agent_id": recipient_agent_id,
                    "token_budget": 500,
                    "query": "evidence first deterministic review process"
                }),
            ))
            .await
            .unwrap();
        let body = to_bytes(recipient_allocation.into_body(), usize::MAX)
            .await
            .unwrap();
        let allocation: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(allocation["selected"].as_array().unwrap().is_empty());
    }
}
