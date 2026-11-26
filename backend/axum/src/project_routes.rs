use crate::{
    auth_middleware::AuthenticatedUser,
    model::{AddWorkspaceMember, CreateProject, Project, ProjectMembership, ProjectRole, SetProjectMember, WorkspaceMembership},
    storage::StorageError,
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
struct ProjectApiError {
    error: &'static str,
}

type ApiResult<T> = Result<T, (StatusCode, Json<ProjectApiError>)>;

fn error(status: StatusCode, message: &'static str) -> (StatusCode, Json<ProjectApiError>) {
    (status, Json(ProjectApiError { error: message }))
}

async fn require_workspace_member(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Uuid,
) -> ApiResult<()> {
    let member = state
        .is_workspace_member(user_id, workspace_id)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace authorization could not be checked."))?;
    if !member {
        return Err(error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    Ok(())
}


pub async fn add_workspace_member(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<AddWorkspaceMember>,
) -> ApiResult<(StatusCode, Json<WorkspaceMembership>)> {
    require_workspace_member(&state, user.user_id, workspace_id).await?;
    let membership = state
        .add_workspace_member(workspace_id, user.user_id, input.user_id)
        .await
        .map_err(|failure| match failure {
            StorageError::ProjectAuthorizationDenied => error(StatusCode::FORBIDDEN, "Only a workspace owner can add members."),
            StorageError::ProjectWorkspaceMembershipRequired => error(StatusCode::NOT_FOUND, "User account was not found."),
            _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace membership could not be added."),
        })?;
    Ok((StatusCode::CREATED, Json(membership)))
}

pub async fn revoke_workspace_member(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    require_workspace_member(&state, user.user_id, workspace_id).await?;
    let revoked = state
        .revoke_workspace_member(workspace_id, user.user_id, target_user_id)
        .await
        .map_err(|failure| match failure {
            StorageError::ProjectAuthorizationDenied => error(StatusCode::FORBIDDEN, "Only a workspace owner can revoke members."),
            _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace membership could not be revoked."),
        })?;
    if !revoked {
        return Err(error(StatusCode::NOT_FOUND, "Revocable workspace membership was not found."));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_project(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<CreateProject>,
) -> ApiResult<(StatusCode, Json<Project>)> {
    require_workspace_member(&state, user.user_id, workspace_id).await?;
    let name = input.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(error(StatusCode::BAD_REQUEST, "Project name must contain between 1 and 120 characters."));
    }
    let project = state
        .create_project_for_user(workspace_id, user.user_id, name.to_owned())
        .await
        .map_err(|failure| match failure {
            StorageError::ProjectAuthorizationDenied => error(StatusCode::FORBIDDEN, "Only a workspace owner can create a project."),
            _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Project could not be created."),
        })?;
    Ok((StatusCode::CREATED, Json(project)))
}

pub async fn list_projects(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<Vec<Project>>> {
    require_workspace_member(&state, user.user_id, workspace_id).await?;
    let projects = state
        .list_user_projects(workspace_id, user.user_id)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Projects could not be listed."))?;
    Ok(Json(projects))
}

pub async fn set_project_member(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, project_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<SetProjectMember>,
) -> ApiResult<(StatusCode, Json<ProjectMembership>)> {
    require_workspace_member(&state, user.user_id, workspace_id).await?;
    let membership = state
        .set_project_member(
            workspace_id,
            project_id,
            user.user_id,
            input.user_id,
            input.role,
        )
        .await
        .map_err(|failure| match failure {
            StorageError::ProjectNotFound => error(StatusCode::NOT_FOUND, "Project not found."),
            StorageError::ProjectAuthorizationDenied => error(StatusCode::FORBIDDEN, "Project role does not permit membership changes."),
            StorageError::ProjectWorkspaceMembershipRequired => error(StatusCode::BAD_REQUEST, "The user must first be an active member of the workspace."),
            _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Project membership could not be changed."),
        })?;
    Ok((StatusCode::OK, Json(membership)))
}

pub async fn revoke_project_member(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, project_id, target_user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    require_workspace_member(&state, user.user_id, workspace_id).await?;
    let revoked = state
        .revoke_project_member(workspace_id, project_id, user.user_id, target_user_id)
        .await
        .map_err(|failure| match failure {
            StorageError::ProjectNotFound => error(StatusCode::NOT_FOUND, "Project not found."),
            StorageError::ProjectAuthorizationDenied => error(StatusCode::FORBIDDEN, "Project role does not permit membership revocation."),
            _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Project membership could not be revoked."),
        })?;
    if !revoked {
        return Err(error(StatusCode::NOT_FOUND, "Active project membership not found."));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app, storage::SqliteStorage};
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn project_creation_and_membership_are_persisted_and_scoped() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let state = AppState::with_storage(storage).await.unwrap();
        let owner = state.ensure_firebase_user("axon-test", "project-owner").await.unwrap();
        let member = state.ensure_firebase_user("axon-test", "project-member").await.unwrap();
        state.add_workspace_member(owner.workspace.id, owner.user_id, member.user_id).await.unwrap();
        let created = state
            .create_project_for_user(owner.workspace.id, owner.user_id, "Research".into())
            .await
            .unwrap();
        assert!(state.is_project_member(owner.workspace.id, created.id, owner.user_id).await.unwrap());
        assert!(!state.is_project_member(owner.workspace.id, created.id, member.user_id).await.unwrap());

        state
            .set_project_member(
                owner.workspace.id,
                created.id,
                owner.user_id,
                member.user_id,
                ProjectRole::Member,
            )
            .await
            .unwrap();
        assert!(state.is_project_member(owner.workspace.id, created.id, member.user_id).await.unwrap());
        assert_eq!(
            state.list_user_projects(owner.workspace.id, member.user_id).await.unwrap().len(),
            1
        );
        assert!(state
            .revoke_project_member(owner.workspace.id, created.id, owner.user_id, member.user_id)
            .await
            .unwrap());
        assert!(!state.is_project_member(owner.workspace.id, created.id, member.user_id).await.unwrap());
    }

    #[tokio::test]
    async fn workspace_revocation_removes_project_access_and_owner_cannot_be_revoked() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let state = AppState::with_storage(storage).await.unwrap();
        let owner = state.ensure_firebase_user("axon-test", "project-owner-3").await.unwrap();
        let member = state.ensure_firebase_user("axon-test", "project-member-3").await.unwrap();
        state.add_workspace_member(owner.workspace.id, owner.user_id, member.user_id).await.unwrap();
        let project = state
            .create_project_for_user(owner.workspace.id, owner.user_id, "Security".into())
            .await
            .unwrap();
        state
            .set_project_member(owner.workspace.id, project.id, owner.user_id, member.user_id, ProjectRole::Member)
            .await
            .unwrap();
        assert!(state.is_project_member(owner.workspace.id, project.id, member.user_id).await.unwrap());
        assert!(state.revoke_workspace_member(owner.workspace.id, owner.user_id, member.user_id).await.unwrap());
        assert!(!state.is_project_member(owner.workspace.id, project.id, member.user_id).await.unwrap());
        assert!(matches!(
            state.revoke_workspace_member(owner.workspace.id, owner.user_id, owner.user_id).await,
            Err(StorageError::ProjectAuthorizationDenied)
        ));
    }

    #[tokio::test]
    async fn project_admin_cannot_grant_admin_or_owner_role() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let state = AppState::with_storage(storage).await.unwrap();
        let owner = state.ensure_firebase_user("axon-test", "project-owner-2").await.unwrap();
        let admin = state.ensure_firebase_user("axon-test", "project-admin-2").await.unwrap();
        let target = state.ensure_firebase_user("axon-test", "project-target-2").await.unwrap();
        state.add_workspace_member(owner.workspace.id, owner.user_id, admin.user_id).await.unwrap();
        state.add_workspace_member(owner.workspace.id, owner.user_id, target.user_id).await.unwrap();
        let project = state
            .create_project_for_user(owner.workspace.id, owner.user_id, "Research".into())
            .await
            .unwrap();
        state
            .set_project_member(owner.workspace.id, project.id, owner.user_id, admin.user_id, ProjectRole::Admin)
            .await
            .unwrap();
        let result = state
            .set_project_member(owner.workspace.id, project.id, admin.user_id, target.user_id, ProjectRole::Owner)
            .await;
        assert!(matches!(result, Err(StorageError::ProjectAuthorizationDenied)));
    }

    #[tokio::test]
    async fn project_endpoints_require_verified_workspace_membership() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let state = AppState::with_storage(storage).await.unwrap();
        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/workspaces/{}/projects", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    }
}
