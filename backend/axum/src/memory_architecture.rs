//! Axon's historical context architecture.
//!
//! Qoin owns semantic vector production and vector-space conventions. Axon owns
//! authorization, QKV attention, event provenance, epoch publication, Pareto
//! context selection, and task-local progressive retrieval.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QoinVector {
    pub values: Vec<f32>,
    pub model_version: String,
}
impl QoinVector {
    pub fn validate(&self) -> Result<(), AttentionError> {
        if self.values.is_empty() || self.values.iter().any(|v| !v.is_finite()) {
            return Err(AttentionError::InvalidVector);
        }
        if self.model_version.trim().is_empty() {
            return Err(AttentionError::MissingModelVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionMemory {
    pub memory_id: Uuid,
    pub key: QoinVector,
    /// The value is a pointer/payload reference, never a lossy replacement for
    /// the immutable event record.
    pub value: String,
    pub outcome_score: f32,
    pub evidence_score: f32,
    pub source_event_sequence: i64,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionQuery {
    pub query: QoinVector,
    pub heads: usize,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionScore {
    pub memory_id: Uuid,
    pub attention: f64,
    pub outcome: f64,
    pub evidence: f64,
    pub tag_overlap: f64,
    pub dependency_overlap: f64,
    pub total: f64,
    pub source_event_sequence: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoPolicy {
    /// The 80/20 principle is a budget heuristic, not a guaranteed law.
    pub initial_share: f64,
    pub target_contribution_coverage: f64,
}
impl Default for ParetoPolicy {
    fn default() -> Self {
        Self { initial_share: 0.20, target_contribution_coverage: 0.80 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoSelection {
    pub initial: Vec<AttentionScore>,
    pub deferred: Vec<AttentionScore>,
    pub achieved_coverage: f64,
    pub budget_exhausted: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AttentionError {
    #[error("query and key vectors must be non-empty, finite, and dimensionally equal")]
    InvalidVector,
    #[error("query dimension must be divisible by the number of attention heads")]
    InvalidHeadCount,
    #[error("vector model version is required")]
    MissingModelVersion,
    #[error("memory score metadata must be finite and in [0, 1]")]
    InvalidScoreMetadata,
    #[error("Pareto policy values must be in (0, 1]")]
    InvalidParetoPolicy,
}

/// Multi-head scaled dot-product attention over Qoin keys. Each head attends
/// to a disjoint vector slice; head probabilities are averaged. The returned
/// value is a ranked set of source references, not synthesized chat content.
pub fn qkv_attention(query: &AttentionQuery, memories: &[AttentionMemory]) -> Result<Vec<AttentionScore>, AttentionError> {
    query.query.validate()?;
    if query.heads == 0 || query.query.values.len() % query.heads != 0 {
        return Err(AttentionError::InvalidHeadCount);
    }
    let mut scored = Vec::with_capacity(memories.len());
    for memory in memories {
        memory.key.validate()?;
        if memory.key.values.len() != query.query.values.len() {
            return Err(AttentionError::InvalidVector);
        }
        if !memory.outcome_score.is_finite() || !(0.0..=1.0).contains(&memory.outcome_score)
            || !memory.evidence_score.is_finite() || !(0.0..=1.0).contains(&memory.evidence_score) {
            return Err(AttentionError::InvalidScoreMetadata);
        }
        let width = query.query.values.len() / query.heads;
        let mut attention = 0.0;
        for head in 0..query.heads {
            let start = head * width;
            let end = start + width;
            let dot = query.query.values[start..end].iter()
                .zip(&memory.key.values[start..end])
                .map(|(q, k)| *q as f64 * *k as f64).sum::<f64>();
            attention += dot / (width as f64).sqrt();
        }
        attention /= query.heads as f64;
        let tag_overlap = jaccard(&query.tags, &memory.tags);
        let dependency_overlap = jaccard(&query.dependency_ids, &memory.dependency_ids);
        // Attention is a logit; sigmoid maps it to [0, 1] before combining
        // independently observed outcome and evidence signals.
        let attention = sigmoid(attention);
        let outcome = memory.outcome_score as f64;
        let evidence = memory.evidence_score as f64;
        let total = 0.65 * attention + 0.15 * outcome + 0.10 * evidence
            + 0.05 * tag_overlap + 0.05 * dependency_overlap;
        scored.push(AttentionScore { memory_id: memory.memory_id, attention, outcome, evidence,
            tag_overlap, dependency_overlap, total, source_event_sequence: memory.source_event_sequence });
    }
    scored.sort_by(|a,b| b.total.total_cmp(&a.total)
        .then_with(|| b.source_event_sequence.cmp(&a.source_event_sequence))
        .then_with(|| a.memory_id.as_bytes().cmp(b.memory_id.as_bytes())));
    Ok(scored)
}

/// Select the highest-contribution initial context under a strict context
/// budget. The target coverage is measured over non-negative ranked scores;
/// when the budget cannot reach 80%, the achieved coverage is reported rather
/// than pretending the Pareto rule was satisfied.
pub fn pareto_select(ranked: &[AttentionScore], policy: &ParetoPolicy) -> Result<ParetoSelection, AttentionError> {
    if !(0.0..1.0).contains(&policy.initial_share)
        || !(0.0..1.0).contains(&policy.target_contribution_coverage) {
        return Err(AttentionError::InvalidParetoPolicy);
    }
    if ranked.is_empty() {
        return Ok(ParetoSelection { initial: vec![], deferred: vec![], achieved_coverage: 1.0, budget_exhausted: false });
    }
    let total: f64 = ranked.iter().map(|s| s.total.max(0.0)).sum();
    let budget = ((ranked.len() as f64 * policy.initial_share).ceil() as usize).max(1).min(ranked.len());
    let mut selected = Vec::new();
    let mut contribution = 0.0;
    for score in ranked.iter().take(budget) {
        selected.push(score.clone());
        contribution += score.total.max(0.0);
        if total == 0.0 || contribution / total >= policy.target_contribution_coverage { break; }
    }
    let selected_ids: BTreeSet<_> = selected.iter().map(|s| s.memory_id).collect();
    let deferred = ranked.iter().filter(|s| !selected_ids.contains(&s.memory_id)).cloned().collect();
    let achieved = if total == 0.0 { 1.0 } else { (contribution / total).clamp(0.0, 1.0) };
    Ok(ParetoSelection { budget_exhausted: achieved < policy.target_contribution_coverage,
        initial: selected, deferred, achieved_coverage: achieved })
}

fn sigmoid(value: f64) -> f64 {
    if value >= 0.0 { 1.0 / (1.0 + (-value).exp()) }
    else { let e = value.exp(); e / (1.0 + e) }
}
fn jaccard<T: Ord>(a: &BTreeSet<T>, b: &BTreeSet<T>) -> f64 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    a.intersection(b).count() as f64 / a.union(b).count() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    fn q(values: Vec<f32>) -> QoinVector { QoinVector { values, model_version: "qoin-test-v1".into() } }
    fn memory(id: Uuid, values: Vec<f32>, outcome: f32) -> AttentionMemory {
        AttentionMemory { memory_id: id, key: q(values), value: format!("memory:{id}"),
            outcome_score: outcome, evidence_score: 0.9, source_event_sequence: 1,
            tags: BTreeSet::new(), dependency_ids: BTreeSet::new() }
    }
    #[test]
    fn multi_head_attention_ranks_matching_keys() {
        let id = Uuid::new_v4();
        let query = AttentionQuery { query: q(vec![1.0, 0.0, 1.0, 0.0]), heads: 2,
            tags: BTreeSet::new(), dependency_ids: BTreeSet::new() };
        let ranked = qkv_attention(&query, &[memory(Uuid::new_v4(), vec![0.0, 1.0, 0.0, 1.0], 1.0),
            memory(id, vec![1.0, 0.0, 1.0, 0.0], 0.8)]).unwrap();
        assert_eq!(ranked[0].memory_id, id);
    }
    #[test]
    fn pareto_selection_respects_budget_and_reports_real_coverage() {
        let scores = (0..10).map(|i| AttentionScore { memory_id: Uuid::new_v4(),
            attention: 0.0, outcome: 0.0, evidence: 0.0, tag_overlap: 0.0,
            dependency_overlap: 0.0, total: if i == 0 { 9.0 } else { 1.0 },
            source_event_sequence: i }).collect::<Vec<_>>();
        let selection = pareto_select(&scores, &ParetoPolicy::default()).unwrap();
        assert_eq!(selection.initial.len(), 2);
        assert!(selection.achieved_coverage >= 0.8);
        assert_eq!(selection.deferred.len(), 8);
    }
}

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use std::time::Duration;

/// SQLite-backed epoch publisher. A separate process may run this coordinator;
/// publication is idempotent per workspace/hour through a unique epoch number.
#[derive(Clone)]
pub struct MemoryEpochCoordinator {
    pool: SqlitePool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEpoch {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub epoch_number: i64,
    pub published_at: DateTime<Utc>,
    pub source_event_sequence: i64,
    pub indexed_memory_count: i64,
    pub snapshot_digest: String,
}
impl MemoryEpochCoordinator {
    pub fn new(pool: SqlitePool) -> Self { Self { pool } }

    /// Publish due workspaces. Called repeatedly by the supervised hourly
    /// worker; the one-hour guard is enforced transactionally per workspace.
    pub async fn publish_due_epochs(&self) -> Result<usize, sqlx::Error> {
        let workspaces: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT workspace_id FROM chats ORDER BY workspace_id"
        ).fetch_all(&self.pool).await?;
        let mut published = 0;
        for workspace in workspaces {
            if self.publish_workspace_if_due(&workspace).await? { published += 1; }
        }
        Ok(published)
    }

    async fn publish_workspace_if_due(&self, workspace_id: &str) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let latest: Option<String> = sqlx::query_scalar(
            "SELECT published_at FROM memory_epochs WHERE workspace_id = ? ORDER BY epoch_number DESC LIMIT 1"
        ).bind(workspace_id).fetch_optional(&mut *tx).await?;
        if let Some(timestamp) = latest {
            let parsed = DateTime::parse_from_rfc3339(&timestamp)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?.with_timezone(&Utc);
            if Utc::now().signed_duration_since(parsed).num_seconds() < 3600 {
                tx.rollback().await?;
                return Ok(false);
            }
        }
        let max_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) FROM memory_events WHERE workspace_id = ?"
        ).bind(workspace_id).fetch_one(&mut *tx).await?;
        let indexed_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_vector_index WHERE workspace_id = ? AND status = 'approved'"
        ).bind(workspace_id).fetch_one(&mut *tx).await?;
        let next_epoch: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(epoch_number), 0) + 1 FROM memory_epochs WHERE workspace_id = ?"
        ).bind(workspace_id).fetch_one(&mut *tx).await?;
        let now = Utc::now();
        let id = Uuid::new_v4();
        // This is a reproducibility marker, not a cryptographic integrity hash.
        let marker = format!("source-sequence:{max_sequence};indexed-memories:{indexed_count}");
        sqlx::query(
            "INSERT INTO memory_epochs (id, workspace_id, epoch_number, published_at, source_event_sequence, indexed_memory_count, snapshot_digest) VALUES (?, ?, ?, ?, ?, ?, ?)"
        ).bind(id.to_string()).bind(workspace_id).bind(next_epoch)
            .bind(now.to_rfc3339()).bind(max_sequence).bind(indexed_count).bind(marker)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn latest_epoch(&self, workspace_id: Uuid) -> Result<Option<MemoryEpoch>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, workspace_id, epoch_number, published_at, source_event_sequence, indexed_memory_count, snapshot_digest FROM memory_epochs WHERE workspace_id = ? ORDER BY epoch_number DESC LIMIT 1"
        ).bind(workspace_id.to_string()).fetch_optional(&self.pool).await?;
        row.map(|row| {
            let id: String = row.try_get("id")?;
            let workspace: String = row.try_get("workspace_id")?;
            let published_at: String = row.try_get("published_at")?;
            Ok(MemoryEpoch {
                id: Uuid::parse_str(&id).map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                workspace_id: Uuid::parse_str(&workspace).map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                epoch_number: row.try_get("epoch_number")?,
                published_at: DateTime::parse_from_rfc3339(&published_at)
                    .map_err(|error| sqlx::Error::Decode(Box::new(error)))?.with_timezone(&Utc),
                source_event_sequence: row.try_get("source_event_sequence")?,
                indexed_memory_count: row.try_get("indexed_memory_count")?,
                snapshot_digest: row.try_get("snapshot_digest")?,
            })
        }).transpose()
    }

    /// Run as a supervised Tokio task. A minute poll avoids a long-running
    /// fixed sleep and recovers naturally after process restarts.
    pub async fn run_hourly(self) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            match self.publish_due_epochs().await {
                Ok(count) if count > 0 => tracing::info!(published_workspaces = count, "published memory epochs"),
                Ok(_) => {}
                Err(error) => tracing::error!(%error, "memory epoch publication failed; will retry"),
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexMemoryInput {
    pub workspace_id: Uuid,
    pub chat_id: Uuid,
    pub memory_id: Uuid,
    pub task_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub owner_agent_id: Option<Uuid>,
    pub visibility: String,
    pub vector: QoinVector,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
    pub outcome_score: f32,
    pub evidence_score: f32,
}
#[derive(Debug, Clone, Serialize)]
pub struct HandoverMemory {
    pub memory_id: Uuid,
    pub source_event_sequence: i64,
    pub payload_json: String,
    pub attention_score: f64,
}
#[derive(Debug, Clone, Serialize)]
pub struct HandoverContext {
    pub session_id: Uuid,
    pub epoch: MemoryEpoch,
    pub initial: Vec<HandoverMemory>,
    pub deferred_count: usize,
    pub achieved_coverage: f64,
    pub budget_exhausted: bool,
}
#[derive(Debug, Clone, Deserialize)]
pub struct HandoverRequest {
    pub workspace_id: Uuid,
    pub agent_id: Uuid,
    pub task_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub query: QoinVector,
    pub heads: usize,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
    #[serde(default)]
    pub pareto: ParetoPolicy,
}

impl MemoryEpochCoordinator {
    /// Accept only vectors attached to an approved, existing source memory.
    /// The original event payload remains in memory_events and is copied as a
    /// retrieval value only for convenient hydration.
    pub async fn index_approved_memory(&self, input: IndexMemoryInput) -> Result<(), sqlx::Error> {
        input.vector.validate().map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        if !["agent_private", "task_shared", "project_shared", "workspace_shared"].contains(&input.visibility.as_str())
            || !input.outcome_score.is_finite() || !(0.0..=1.0).contains(&input.outcome_score)
            || !input.evidence_score.is_finite() || !(0.0..=1.0).contains(&input.evidence_score) {
            return Err(sqlx::Error::Protocol("invalid memory index metadata".into()));
        }
        if input.visibility == "agent_private" && input.owner_agent_id.is_none()
            || input.visibility == "task_shared" && input.task_id.is_none()
            || input.visibility == "project_shared" && input.project_id.is_none() {
            return Err(sqlx::Error::Protocol("visibility scope identifier is required".into()));
        }
        let source = sqlx::query(
            "SELECT sequence, payload_json FROM memory_events WHERE workspace_id = ? AND chat_id = ? AND memory_entry_id = ? ORDER BY sequence DESC LIMIT 1"
        ).bind(input.workspace_id.to_string()).bind(input.chat_id.to_string())
            .bind(input.memory_id.to_string()).fetch_optional(&self.pool).await?;
        let Some(source) = source else { return Err(sqlx::Error::RowNotFound); };
        let payload: String = source.try_get("payload_json")?;
        let parsed: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        if parsed.get("status").and_then(|v| v.as_str()) != Some("approved") {
            return Err(sqlx::Error::Protocol("only approved memories may be indexed".into()));
        }
        let sequence: i64 = source.try_get("sequence")?;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO memory_vector_index (memory_id, workspace_id, chat_id, task_id, project_id, owner_agent_id, visibility, status, qoin_vector_json, key_metadata_json, value_payload_json, tags_json, dependency_ids_json, outcome_score, evidence_score, source_event_sequence, model_version, indexed_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'approved', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(memory_id) DO UPDATE SET workspace_id=excluded.workspace_id, chat_id=excluded.chat_id, task_id=excluded.task_id, project_id=excluded.project_id, owner_agent_id=excluded.owner_agent_id, visibility=excluded.visibility, status='approved', qoin_vector_json=excluded.qoin_vector_json, key_metadata_json=excluded.key_metadata_json, value_payload_json=excluded.value_payload_json, tags_json=excluded.tags_json, dependency_ids_json=excluded.dependency_ids_json, outcome_score=excluded.outcome_score, evidence_score=excluded.evidence_score, source_event_sequence=excluded.source_event_sequence, model_version=excluded.model_version, indexed_at=excluded.indexed_at"
        ).bind(input.memory_id.to_string()).bind(input.workspace_id.to_string())
            .bind(input.chat_id.to_string()).bind(input.task_id.map(|v| v.to_string()))
            .bind(input.project_id.map(|v| v.to_string())).bind(input.owner_agent_id.map(|v| v.to_string()))
            .bind(input.visibility).bind(serde_json::to_string(&input.vector).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
            .bind(serde_json::json!({"tags": &input.tags, "dependencies": &input.dependency_ids}).to_string())
            .bind(payload).bind(serde_json::to_string(&input.tags).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
            .bind(serde_json::to_string(&input.dependency_ids).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
            .bind(input.outcome_score).bind(input.evidence_score).bind(sequence)
            .bind(input.vector.model_version).bind(now).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn prepare_handover(&self, request: HandoverRequest) -> Result<HandoverContext, sqlx::Error> {
        request.query.validate().map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        let epoch = self.latest_epoch(request.workspace_id).await?.ok_or(sqlx::Error::RowNotFound)?;
        let rows = sqlx::query(
            "SELECT memory_id, qoin_vector_json, value_payload_json, tags_json, dependency_ids_json, outcome_score, evidence_score, source_event_sequence FROM memory_vector_index WHERE workspace_id = ? AND status = 'approved' AND (visibility = 'workspace_shared' OR (visibility = 'agent_private' AND owner_agent_id = ?) OR (visibility = 'task_shared' AND task_id = ?) OR (visibility = 'project_shared' AND project_id = ?)) AND source_event_sequence <= ? ORDER BY source_event_sequence DESC"
        ).bind(request.workspace_id.to_string()).bind(request.agent_id.to_string())
            .bind(request.task_id.map(|v| v.to_string())).bind(request.project_id.map(|v| v.to_string()))
            .bind(epoch.source_event_sequence).fetch_all(&self.pool).await?;
        let mut memories = Vec::with_capacity(rows.len());
        let mut payloads = std::collections::HashMap::new();
        for row in rows {
            let id_text: String = row.try_get("memory_id")?;
            let id = Uuid::parse_str(&id_text).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            let vector_json: String = row.try_get("qoin_vector_json")?;
            let vector: QoinVector = serde_json::from_str(&vector_json).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            if vector.model_version != request.query.model_version {
                continue;
            }
            let tags_json: String = row.try_get("tags_json")?;
            let tags: BTreeSet<String> = serde_json::from_str(&tags_json).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            let deps_json: String = row.try_get("dependency_ids_json")?;
            let deps_vec: Vec<Uuid> = serde_json::from_str(&deps_json).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            let dependencies = deps_vec.into_iter().collect::<BTreeSet<_>>();
            let value: String = row.try_get("value_payload_json")?;
            let outcome: f32 = row.try_get("outcome_score")?;
            let evidence: f32 = row.try_get("evidence_score")?;
            let sequence: i64 = row.try_get("source_event_sequence")?;
            payloads.insert(id, (value, sequence));
            memories.push(AttentionMemory { memory_id: id, key: vector, value: id.to_string(),
                outcome_score: outcome, evidence_score: evidence, source_event_sequence: sequence,
                tags, dependency_ids: dependencies });
        }
        let query = AttentionQuery { query: request.query, heads: request.heads,
            tags: request.tags, dependency_ids: request.dependency_ids };
        let ranked = qkv_attention(&query, &memories).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        let selection = pareto_select(&ranked, &request.pareto).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        let session_id = Uuid::new_v4();
        let initial_ids = selection.initial.iter().map(|s| s.memory_id).collect::<Vec<_>>();
        sqlx::query(
            "INSERT INTO agent_memory_sessions (session_id, workspace_id, agent_id, task_id, memory_epoch_id, initial_memory_ids_json, retrieved_memory_ids_json, created_at) VALUES (?, ?, ?, ?, ?, ?, '[]', ?)"
        ).bind(session_id.to_string()).bind(request.workspace_id.to_string())
            .bind(request.agent_id.to_string()).bind(request.task_id.map(|v| v.to_string()))
            .bind(epoch.id.to_string()).bind(serde_json::to_string(&initial_ids).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
            .bind(Utc::now().to_rfc3339()).execute(&self.pool).await?;
        let initial = selection.initial.iter().filter_map(|score| {
            payloads.get(&score.memory_id).map(|(payload, sequence)| HandoverMemory {
                memory_id: score.memory_id, source_event_sequence: *sequence,
                payload_json: payload.clone(), attention_score: score.total
            })
        }).collect();
        Ok(HandoverContext { session_id, epoch, initial, deferred_count: selection.deferred.len(),
            achieved_coverage: selection.achieved_coverage, budget_exhausted: selection.budget_exhausted })
    }
}
