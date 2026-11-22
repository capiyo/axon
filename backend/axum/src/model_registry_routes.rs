use crate::{
    auth_middleware::AuthenticatedUser,
    audit::AuditEvent,
    model_registry::{
        AssignmentDecision, ModelEvaluation, ModelProvider, ModelRecord, ModelStat,
        ProviderDataTerms, RegistryStatus,
    },
    storage::{EntityKind, StoredEntity},
    AppState,
};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};
use uuid::Uuid;

const MIN_APPROVAL_SCORE: f64 = 0.80;

#[derive(Debug, Serialize)]
pub struct AdminError { error: String }

type AdminResult<T> = Result<T, (StatusCode, Json<AdminError>)>;

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<AdminError>) {
    (status, Json(AdminError { error: message.into() }))
}

fn require_admin(user: &AuthenticatedUser) -> AdminResult<()> {
    if user.is_platform_admin { Ok(()) } else { Err(error(StatusCode::FORBIDDEN, "Platform administrator access required.")) }
}

#[derive(Debug, Deserialize)]
pub struct CreateProvider {
    pub name: String,
    pub base_url: String,
    #[serde(default = "active_status")]
    pub status: String,
}
fn active_status() -> String { "active".into() }

#[derive(Debug, Deserialize)]
pub struct ReviewProviderTerms {
    pub processing_regions: Vec<String>,
    pub trains_on_api_inputs: Option<bool>,
    pub retention_days: Option<u32>,
    pub retention_description: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateModel {
    pub provider_id: Uuid,
    pub model_name: String,
    pub abilities: BTreeSet<String>,
    pub categories: BTreeSet<String>,
    pub context_length: u64,
    pub input_cost_per_million: u64,
    pub output_cost_per_million: u64,
    pub typical_latency_ms: u64,
    pub regions: BTreeSet<String>,
    pub version: Option<String>,
    pub notes: String,
    #[serde(default)]
    pub curated_rank: BTreeMap<String, u16>,
}

#[derive(Debug, Deserialize)]
pub struct CandidateReview {
    pub action: String,
    pub note: String,
}

async fn persist(
    state: &AppState,
    user: &AuthenticatedUser,
    entity: StoredEntity,
    event_type: &str,
    entity_type: &str,
    id: Uuid,
    metadata: serde_json::Value,
) -> AdminResult<()> {
    state.persist_entity(entity, Vec::new(), AuditEvent::attributed(
        format!("human:{}", user.user_id), None, event_type, entity_type, id.to_string(), metadata,
    )).await.map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Registry mutation could not be persisted."))?;
    Ok(())
}

pub async fn list_providers(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
) -> AdminResult<Json<Vec<ModelProvider>>> {
    require_admin(&user)?;
    let entities = state.list_entities(EntityKind::Provider, None).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Providers could not be loaded."))?;
    Ok(Json(entities.into_iter().filter_map(|entity| if let StoredEntity::Provider(item) = entity { Some(item) } else { None }).collect()))
}

pub async fn create_provider(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Json(input): Json<CreateProvider>,
) -> AdminResult<(StatusCode, Json<ModelProvider>)> {
    require_admin(&user)?;
    if input.name.trim().is_empty() || input.name.len() > 120
        || !(input.base_url.starts_with("https://") || input.base_url.starts_with("http://localhost"))
        || input.status != "active" && input.status != "disabled"
    {
        return Err(error(StatusCode::BAD_REQUEST, "Provider name, HTTPS base URL and status are invalid."));
    }
    let now = Utc::now();
    let provider = ModelProvider {
        id: Uuid::new_v4(), name: input.name.trim().into(), base_url: input.base_url.trim().into(),
        status: input.status, data_terms: ProviderDataTerms {
            processing_regions: Vec::new(), trains_on_api_inputs: None, retention_days: None,
            retention_description: "Not reviewed".into(),
        },
        terms_reviewed_at: None, reviewed_by: None, created_at: now, updated_at: now,
    };
    persist(&state, &user, StoredEntity::Provider(provider.clone()), "model_provider.created",
        "model_provider", provider.id, json!({"name": provider.name, "base_url": provider.base_url})).await?;
    Ok((StatusCode::CREATED, Json(provider)))
}

pub async fn review_provider_terms(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(provider_id): Path<Uuid>,
    Json(input): Json<ReviewProviderTerms>,
) -> AdminResult<Json<ModelProvider>> {
    require_admin(&user)?;
    if input.processing_regions.is_empty() || input.retention_description.trim().is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "Processing regions and retention description are required."));
    }
    let mut providers: Vec<ModelProvider> = state.list_entities(EntityKind::Provider, None).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Providers could not be loaded."))?
        .into_iter().filter_map(|entity| if let StoredEntity::Provider(item) = entity { Some(item) } else { None }).collect();
    let provider = providers.iter_mut().find(|item| item.id == provider_id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Provider not found."))?;
    provider.data_terms = ProviderDataTerms {
        processing_regions: input.processing_regions, trains_on_api_inputs: input.trains_on_api_inputs,
        retention_days: input.retention_days, retention_description: input.retention_description.trim().into(),
    };
    provider.terms_reviewed_at = Some(Utc::now());
    provider.reviewed_by = Some(user.user_id);
    provider.updated_at = Utc::now();
    let updated = provider.clone();
    persist(&state, &user, StoredEntity::Provider(updated.clone()), "model_provider.terms_reviewed",
        "model_provider", provider_id, json!({"reviewed_by": user.user_id})).await?;
    Ok(Json(updated))
}

pub async fn list_models(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
) -> AdminResult<Json<Vec<ModelRecord>>> {
    require_admin(&user)?;
    let entities = state.list_entities(EntityKind::Model, None).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Models could not be loaded."))?;
    Ok(Json(entities.into_iter().filter_map(|entity| if let StoredEntity::Model(item) = entity { Some(item) } else { None }).collect()))
}

pub async fn create_model(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Json(input): Json<CreateModel>,
) -> AdminResult<(StatusCode, Json<ModelRecord>)> {
    require_admin(&user)?;
    if input.model_name.trim().is_empty() || input.abilities.is_empty() || input.categories.is_empty()
        || input.context_length == 0 || input.notes.len() > 4000
    {
        return Err(error(StatusCode::BAD_REQUEST, "Model name, abilities, categories and context length are required."));
    }
    let providers = state.list_entities(EntityKind::Provider, None).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Providers could not be loaded."))?;
    if !providers.iter().any(|entity| matches!(entity, StoredEntity::Provider(provider) if provider.id == input.provider_id && provider.status == "active")) {
        return Err(error(StatusCode::BAD_REQUEST, "An active provider must exist before registering a model."));
    }
    let now = Utc::now();
    let model = ModelRecord {
        id: Uuid::new_v4(), provider_id: input.provider_id, model_name: input.model_name.trim().into(),
        status: RegistryStatus::Candidate, abilities: input.abilities, categories: input.categories,
        context_length: input.context_length, input_cost_per_million: input.input_cost_per_million,
        output_cost_per_million: input.output_cost_per_million, typical_latency_ms: input.typical_latency_ms,
        regions: input.regions, version: input.version, last_seen_at: now, notes: input.notes,
        curated_rank: input.curated_rank, healthy: false, flagged_for_review: false,
        created_at: now, updated_at: now,
    };
    persist(&state, &user, StoredEntity::Model(model.clone()), "model.candidate_created",
        "model", model.id, json!({"provider_id": model.provider_id, "status": "candidate"})).await?;
    Ok((StatusCode::CREATED, Json(model)))
}

pub async fn review_candidate(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(model_id): Path<Uuid>,
    Json(input): Json<CandidateReview>,
) -> AdminResult<Json<ModelRecord>> {
    require_admin(&user)?;
    if input.note.trim().is_empty() || input.note.len() > 2000 || !["approve", "reject"].contains(&input.action.as_str()) {
        return Err(error(StatusCode::BAD_REQUEST, "A valid approve/reject action and a review note are required."));
    }
    let mut models: Vec<ModelRecord> = state.list_entities(EntityKind::Model, None).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Models could not be loaded."))?
        .into_iter().filter_map(|entity| if let StoredEntity::Model(item) = entity { Some(item) } else { None }).collect();
    let model = models.iter_mut().find(|item| item.id == model_id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Model not found."))?;
    if model.status != RegistryStatus::Candidate {
        return Err(error(StatusCode::CONFLICT, "Only candidate models can be reviewed."));
    }
    if input.action == "approve" {
        let providers = state.list_entities(EntityKind::Provider, None).await
            .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Provider terms could not be loaded."))?;
        let terms_reviewed = providers.iter().any(|entity| matches!(entity, StoredEntity::Provider(provider)
            if provider.id == model.provider_id && provider.terms_reviewed_at.is_some() && provider.reviewed_by.is_some()));
        if !terms_reviewed {
            return Err(error(StatusCode::CONFLICT, "Provider data terms must be explicitly reviewed before model approval."));
        }
        let evaluations: Vec<ModelEvaluation> = state.list_entities(EntityKind::ModelEvaluation, None).await
            .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Model evaluations could not be loaded."))?
            .into_iter().filter_map(|entity| if let StoredEntity::ModelEvaluation(item) = entity { Some(item) } else { None }).collect();
        for category in &model.categories {
            let latest = evaluations.iter().filter(|evaluation| evaluation.model_id == model_id && evaluation.category == *category)
                .max_by_key(|evaluation| evaluation.run_at);
            if !latest.is_some_and(|evaluation| evaluation.score >= MIN_APPROVAL_SCORE) {
                return Err(error(StatusCode::CONFLICT, "Every supported category must pass the current evaluation threshold before approval."));
            }
        }
        model.status = RegistryStatus::Approved;
        model.healthy = true;
        model.flagged_for_review = false;
    } else {
        model.status = RegistryStatus::Disabled;
        model.healthy = false;
    }
    model.updated_at = Utc::now();
    let updated = model.clone();
    persist(&state, &user, StoredEntity::Model(updated.clone()), if input.action == "approve" { "model.approved" } else { "model.rejected" },
        "model", model_id, json!({"action": input.action, "note": input.note.trim(), "reviewed_by": user.user_id})).await?;
    Ok(Json(updated))
}

pub async fn list_evaluations(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
) -> AdminResult<Json<Vec<ModelEvaluation>>> {
    require_admin(&user)?;
    let entities = state.list_entities(EntityKind::ModelEvaluation, None).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Evaluations could not be loaded."))?;
    Ok(Json(entities.into_iter().filter_map(|entity| if let StoredEntity::ModelEvaluation(item) = entity { Some(item) } else { None }).collect()))
}

pub async fn list_stats(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
) -> AdminResult<Json<Vec<ModelStat>>> {
    require_admin(&user)?;
    let entities = state.list_entities(EntityKind::ModelStat, None).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Model statistics could not be loaded."))?;
    Ok(Json(entities.into_iter().filter_map(|entity| if let StoredEntity::ModelStat(item) = entity { Some(item) } else { None }).collect()))
}

pub async fn list_assignments(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
) -> AdminResult<Json<Vec<AssignmentDecision>>> {
    require_admin(&user)?;
    let entities = state.list_entities(EntityKind::AssignmentDecision, None).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Assignment decisions could not be loaded."))?;
    Ok(Json(entities.into_iter().filter_map(|entity| if let StoredEntity::AssignmentDecision(item) = entity { Some(item) } else { None }).collect()))
}
