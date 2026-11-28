pub enum RegistryStatus {
    Candidate,
    Approved,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderDataTerms {
    pub processing_regions: Vec<String>,
    pub trains_on_api_inputs: Option<bool>,
    pub retention_days: Option<u32>,
    pub retention_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelProvider {
    pub id: Uuid,
    pub name: String,
    pub base_url: String,
    pub model_discovery_url: Option<String>,
    pub pricing_source_url: Option<String>,
    pub api_key_env: Option<String>,
    pub discovery_enabled: bool,
    pub rate_limit_per_minute: u16,
    pub status: String,
    pub data_terms: ProviderDataTerms,
    pub terms_reviewed_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRecord {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub model_name: String,
    pub status: RegistryStatus,
    pub abilities: BTreeSet<String>,
    pub categories: BTreeSet<String>,
    pub context_length: u64,
    pub input_cost_per_million: u64,
    pub output_cost_per_million: u64,
    pub typical_latency_ms: u64,
    pub regions: BTreeSet<String>,
    pub version: Option<String>,
    pub last_seen_at: DateTime<Utc>,
    pub notes: String,
    pub curated_rank: BTreeMap<String, u16>,
    pub healthy: bool,
    pub flagged_for_review: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelChangeFlag {
    pub id: Uuid,
    pub model_id: Uuid,
    pub change_kind: String,
    pub previous_value: Option<String>,
    pub observed_value: Option<String>,
    pub detected_at: DateTime<Utc>,
    pub cleared_at: Option<DateTime<Utc>>,
    pub cleared_by: Option<Uuid>,
    pub resolution_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelEvaluation {
    pub id: Uuid,
    pub model_id: Uuid,
    pub category: String,
    pub test_set_version: String,
    pub score: f64,
    pub run_at: DateTime<Utc>,
    pub cost_micros: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelStat {
    pub id: Uuid,
    pub model_id: Uuid,
    pub category: String,
    pub role: AssignmentRole,
    pub runs: u64,
    pub completion_rate: f64,
    pub supervisor_pass_rate: f64,
    pub user_retry_rate: f64,
    pub cost_per_run_micros: u64,
    pub latency_ms: u64,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentRole {
    ChecklistWriter,
    Worker,
    Supervisor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLevel {
    Quick,
    Normal,
    Important,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyRequirement {
    pub forbid_training: bool,
    pub forbid_retention: bool,
    pub allowed_regions: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct AssignmentProfile {
    pub run_id: Uuid,
    pub category: String,
    pub level: TaskLevel,
    pub low_risk: bool,
    pub required_abilities: BTreeSet<String>,
    pub required_context_tokens: u64,
    pub privacy: PrivacyRequirement,
    pub estimated_input_tokens: u64,
    pub estimated_output_tokens: u64,
    pub per_task_budget_micros: u64,
    pub daily_budget_remaining_micros: u64,
    pub exploration_percent: u8,
    pub minimum_history_runs: u64,
    pub curated_model_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssignmentDecision {
    pub id: Uuid,
    pub run_id: Uuid,
    pub role: AssignmentRole,
    pub slot: u8,
    pub candidates_considered: Vec<Uuid>,
    pub filters_applied: Vec<String>,
    pub chosen_model_id: Uuid,
    pub reason: String,
    pub estimated_cost_micros: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssignmentPlan {
    pub level: TaskLevel,
    pub decisions: Vec<AssignmentDecision>,
    pub estimated_cost_micros: u64,
    pub budget_downgraded: bool,
    pub exploration_used: bool,
    pub diversity_degraded: bool,
    pub notices: Vec<String>,
}

