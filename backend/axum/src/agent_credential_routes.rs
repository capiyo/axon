use crate::{
    auth_middleware::AuthenticatedUser,
    storage::{IssuedAgentCredential, StorageError},
    AppState,
};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct AgentCredentialResponse {
    pub credential_id: Uuid,
    /// Returned only at issuance/rotation. The database stores only its hash.
    pub token: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<IssuedAgentCredential> for AgentCredentialResponse {
    fn from(value: IssuedAgentCredential) -> Self {
        Self {
            credential_id: value.credential_id,
            token: value.token,
            created_at: value.created_at,
            expires_at: value.expires_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AgentCredentialApiError {
    error: &'static str,
}

type ApiResult<T> = Result<T, (StatusCode, Json<AgentCredentialApiError>)>;

fn error(status: StatusCode, message: &'static str) -> (StatusCode, Json<AgentCredentialApiError>) {
    (status, Json(AgentCredentialApiError { error: message }))
}

async fn require_agent_owner(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Uuid,
    agent_id: Uuid,
) -> ApiResult<()> {
    if state.is_agent_owner(user_id, workspace_id, agent_id).await {
        Ok(())
    } else {
        Err(error(
            StatusCode::NOT_FOUND,
            "Agent not found for this user in the workspace.",
        ))
    }
}

pub async fn issue_agent_credential(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<(StatusCode, Json<AgentCredentialResponse>)> {
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;
    let credential = state
        .issue_agent_credential(workspace_id, agent_id, user.user_id)
        .await
        .map_err(|failure| match failure {
            StorageError::ProjectAuthorizationDenied => error(
                StatusCode::FORBIDDEN,
                "Only the registered agent owner can issue credentials.",
            ),
            StorageError::ProjectNotFound => error(StatusCode::NOT_FOUND, "Agent not found."),
            _ => error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Agent credential could not be issued.",
            ),
        })?;
    Ok((StatusCode::CREATED, Json(credential.into())))
}

pub async fn rotate_agent_credential(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, credential_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<AgentCredentialResponse>> {
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;
    let credential = state
        .rotate_agent_credential(workspace_id, agent_id, credential_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Agent credential could not be rotated.",
            )
        })?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Active agent credential not found."))?;
    Ok(Json(credential.into()))
}

pub async fn revoke_agent_credential(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, credential_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;
    let revoked = state
        .revoke_agent_credential(workspace_id, agent_id, credential_id, user.user_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Agent credential could not be revoked.",
            )
        })?;
    if !revoked {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Active agent credential not found.",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::Agent,
        storage::{SqliteStorage, Storage},
    };
    use chrono::Utc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn agent_tokens_are_limited_to_their_agent_memory_routes() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let state = AppState::with_storage(storage.clone().shared())
            .await
            .unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "agent-route-owner")
            .await
            .unwrap();
        let agent = Agent {
            id: Uuid::new_v4(),
            workspace_id: account.workspace.id,
            created_by_user_id: Some(account.user_id),
            display_name: "Route-bound agent".into(),
            provider: "test".into(),
            connection_mode: crate::model::ConnectionMode::Manual,
            created_at: Utc::now(),
        };
        storage
            .apply(crate::storage::StorageMutation {
                entity: crate::storage::StoredEntity::Agent(agent.clone()),
                related_entities: Vec::new(),
                audit: crate::audit::AuditEvent::attributed(
                    format!("human:{}", account.user_id),
                    Some(account.workspace.id),
                    "agent.registered",
                    "agent",
                    agent.id.to_string(),
                    serde_json::json!({}),
                ),
            })
            .await
            .unwrap();
        state.agents.write().await.insert(agent.id, agent.clone());
        let credential = state
            .issue_agent_credential(account.workspace.id, agent.id, account.user_id)
            .await
            .unwrap();
        let app = crate::app(state);
        let agent_path = format!(
            "/api/v1/workspaces/{}/agents/{}/memory-documents/{}/1",
            account.workspace.id,
            agent.id,
            Uuid::new_v4()
        );
        let accepted = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri(&agent_path)
                    .header(
                        axum::http::header::AUTHORIZATION,
                        format!("Bearer {}", credential.token),
                    )
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            accepted.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );

        let feedback_path = format!(
            "/api/v1/workspaces/{}/agents/{}/memory-sessions/{}/feedback",
            account.workspace.id,
            agent.id,
            Uuid::new_v4()
        );
        let feedback = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(&feedback_path)
                    .header(
                        axum::http::header::AUTHORIZATION,
                        format!("Bearer {}", credential.token),
                    )
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"memory_id":"{}","document_version":1,"event_type":"helpful"}}"#,
                        Uuid::new_v4()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(feedback.status(), axum::http::StatusCode::FORBIDDEN);

        let wrong_agent_path = format!(
            "/api/v1/workspaces/{}/agents/{}/memory-documents/{}/1",
            account.workspace.id,
            Uuid::new_v4(),
            Uuid::new_v4()
        );
        let denied = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri(&wrong_agent_path)
                    .header(
                        axum::http::header::AUTHORIZATION,
                        format!("Bearer {}", credential.token),
                    )
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), axum::http::StatusCode::FORBIDDEN);

        let non_agent_path = format!("/api/v1/workspaces/{}/audit", account.workspace.id);
        let denied = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri(&non_agent_path)
                    .header(
                        axum::http::header::AUTHORIZATION,
                        format!("Bearer {}", credential.token),
                    )
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn credentials_are_hashed_scoped_rotatable_and_revocable() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "agent-credential-owner")
            .await
            .unwrap();
        let agent = Agent {
            id: Uuid::new_v4(),
            workspace_id: account.workspace.id,
            created_by_user_id: Some(account.user_id),
            display_name: "Credential test agent".into(),
            provider: "test".into(),
            connection_mode: crate::model::ConnectionMode::Manual,
            created_at: Utc::now(),
        };
        storage
            .apply(crate::storage::StorageMutation {
                entity: crate::storage::StoredEntity::Agent(agent.clone()),
                related_entities: Vec::new(),
                audit: crate::audit::AuditEvent {
                    sequence: 0,
                    event_id: Uuid::new_v4(),
                    workspace_id: Some(account.workspace.id),
                    actor: format!("human:{}", account.user_id),
                    event_type: "agent.registered".into(),
                    entity_type: "agent".into(),
                    entity_id: agent.id.to_string(),
                    occurred_at: Utc::now(),
                    metadata: serde_json::json!({}),
                },
            })
            .await
            .unwrap();

        let issued = storage
            .issue_agent_credential(account.workspace.id, agent.id, account.user_id)
            .await
            .unwrap();
        let stored_hash: String =
            sqlx::query_scalar("SELECT secret_hash FROM agent_credentials WHERE id = ?")
                .bind(issued.credential_id.to_string())
                .fetch_one(storage.test_pool())
                .await
                .unwrap();
        assert!(!stored_hash.is_empty());
        assert!(!stored_hash.contains(issued.token.split('.').nth(1).unwrap()));

        let identity = storage
            .authenticate_agent_credential(&issued.token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(identity.agent_id, agent.id);
        assert_eq!(identity.owner_user_id, account.user_id);

        let rotated = storage
            .rotate_agent_credential(
                account.workspace.id,
                agent.id,
                issued.credential_id,
                account.user_id,
            )
            .await
            .unwrap()
            .unwrap();
        assert!(storage
            .authenticate_agent_credential(&issued.token)
            .await
            .unwrap()
            .is_none());
        assert!(storage
            .authenticate_agent_credential(&rotated.token)
            .await
            .unwrap()
            .is_some());
        assert!(storage
            .revoke_agent_credential(
                account.workspace.id,
                agent.id,
                rotated.credential_id,
                account.user_id
            )
            .await
            .unwrap());
        assert!(storage
            .authenticate_agent_credential(&rotated.token)
            .await
            .unwrap()
            .is_none());
    }
}
