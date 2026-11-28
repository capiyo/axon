/**
 * Language-neutral API contract types for the Axon web and mobile clients.
 * Keep these aligned with the versioned schemas in /contracts.
 */
export type Workspace = {
  id: string;
  name: string;
  created_at: string;
};

export type Agent = {
  id: string;
  workspace_id: string;
  display_name: string;
  provider: string;
  connection_mode: "manual";
  created_at: string;
};

export type Artifact = {
  id: string;
  workspace_id: string;
  title: string;
  content: string;
  source_agent_id: string | null;
  captured_at: string;
};

export type TaskStatus =
  | "offered"
  | "acknowledged"
  | "in_progress"
  | "submitted"
  | "review_required"
  | "accepted"
  | "changes_requested"
  | "completed"
  | "rejected"
  | "cancelled";

export type TaskTransition = {
  from: TaskStatus | null;
  to: TaskStatus;
  note: string;
  occurred_at: string;
};

export type Task = {
  id: string;
  workspace_id: string;
  title: string;
  objective: string;
  assignee_agent_id: string;
  source_chat_id: string | null;
  context_artifact_ids: string[];
  response_artifact_id: string | null;
  status: TaskStatus;
  version: number;
  created_at: string;
  updated_at: string;
  transitions: TaskTransition[];
};

export type Health = {
  status: string;
  service: string;
  storage: "development_memory_only" | "persistent" | string;
  authentication: string;
};


export type AxonDeviceKind =
  | "mobile_robot" | "robot_arm" | "drone" | "sensor" | "simulator" | "other";

export type DeviceRegistration = {
  device_id: string;
  workspace_id: string;
  device_type: AxonDeviceKind;
  manufacturer: string;
  model: string;
  identity_key_fingerprint: string;
  capabilities: string[];
  firmware_version?: string;
  registered_at: string;
};

export type TelemetryKind =
  | "pose" | "velocity" | "joint_state" | "battery" | "temperature"
  | "safety_state" | "diagnostic" | "heartbeat";

export type TelemetryEvent = {
  event_id: string;
  device_id: string;
  sequence: number;
  observed_at: string;
  received_at: string;
  kind: TelemetryKind;
  frame_id?: string;
  payload: Record<string, unknown>;
  quality?: "measured" | "estimated" | "simulated" | "unknown";
};

export type SimulatorTelemetryIngest = {
  event_id: string;
  sequence: number;
  observed_at: string;
  kind: TelemetryKind;
  frame_id?: string;
  payload: Record<string, unknown>;
  quality: "simulated";
};

export type CommandProposal = {
  proposal_id: string;
  device_id: string;
  requester_id: string;
  operation: "move_base" | "set_joint_target" | "stop" | "home" | "custom";
  parameters: Record<string, unknown>;
  sequence: number;
  issued_at: string;
  expires_at: string;
  idempotency_key: string;
  reason: string;
  simulation_only?: true;
};

export type DeviceSafetyState = {
  device_id: string;
  emergency_stop: boolean;
  interlocks_healthy: boolean;
  operating_mode: "disabled" | "manual" | "assisted" | "autonomous" | "maintenance" | "fault";
  observed_at: string;
  source: "device_controller" | "safety_plc" | "simulator";
  fault_codes?: string[];
};


export type AxonPrincipalKind = "human" | "agent" | "service" | "device";

export type AxonPrincipalRef = {
  kind: AxonPrincipalKind;
  id: string;
};

export type AxonCapability =
  | "workspace_read" | "workspace_manage"
  | "agent_register" | "agent_manage"
  | "artifact_read" | "artifact_create" | "artifact_classify"
  | "task_read" | "task_create" | "task_assign" | "task_review"
  | "decision_propose" | "decision_accept" | "audit_read"
  | "device_enroll" | "device_telemetry_read" | "device_telemetry_write"
  | "device_command_propose";

/** Authorization grant only; never use this as evidence that a caller is authenticated. */
export type AxonCapabilityGrant = {
  grant_id: string;
  subject: AxonPrincipalRef;
  workspace_id: string;
  capabilities: AxonCapability[];
  issued_by: AxonPrincipalRef;
  issued_at: string;
  expires_at: string | null;
  revoked_at: string | null;
};


/** Process-local audit event. Actor is explicitly unattributed until auth is integrated. */
export type AxonAuditEvent = {
  sequence: number;
  event_id: string;
  workspace_id: string | null;
  actor: "system:local-development-unattributed" | string;
  event_type: string;
  entity_type: string;
  entity_id: string;
  occurred_at: string;
  metadata: Record<string, unknown>;
};


export type ModerationReport = {
  id: string;
  listing_id: string;
  comment_id: string | null;
  reporter_id: string;
  reason: string;
  status: string;
  created_at: string;
};

export type Notification = {
  id: string;
  workspace_id: string | null;
  event_type: string;
  entity_type: string;
  entity_id: string;
  title: string;
  body: string;
  created_at: string;
  read_at: string | null;
};

export type NotificationPage = {
  items: Notification[];
  unread_count: number;
};

export type ExploreAgent = {
  id: string;
  agent_id: string;
  workspace_id: string;
  owner_id: string;
  title: string;
  provider: string;
  description: string;
  category: string;
  created_at: string;
  updated_at: string;
  like_count: number;
  comment_count: number;
  is_liked: boolean;
  is_following: boolean;
  is_owner: boolean;
};

export type ExploreComment = {
  id: string;
  listing_id: string;
  parent_comment_id: string | null;
  author_id: string;
  author_label: string;
  body: string;
  created_at: string;
  updated_at: string;
  like_count: number;
  reply_count: number;
  is_liked: boolean;
  is_mine: boolean;
};

export type Chat = {
  id: string;
  workspace_id: string;
  owner_id: string;
  title: string;
  created_at: string;
  updated_at: string;
  archived: boolean;
  unread_count: number;
  is_liked: boolean;
  message_count: number;
  last_message: ChatMessage | null;
};

export type ChatMessage = {
  id: string;
  chat_id: string;
  role: "user" | "assistant" | "system";
  content: string;
  created_at: string;
  answered_by: string | null;
};

export type ImportedMessage = {
  id: string | null;
  role: "user" | "assistant" | "system";
  content: string;
  created_at: string | null;
};

export type ConversationImport = {
  id: string;
  workspace_id: string;
  provider: string;
  conversation_id: string;
  title: string;
  messages: ImportedMessage[];
  content_sha256: string;
  message_count: number;
  imported_at: string;
};

export type ImportConversationRequest = {
  provider: string;
  conversation_id: string;
  title: string;
  messages: ImportedMessage[];
};

export type MemoryKind =
  | "semantic"
  | "episodic"
  | "project"
  | "decision"
  | "preference"
  | "procedural";

export type MemoryStatus = "proposed" | "active" | "rejected" | "superseded";
export type MemoryReviewDecision = "approve" | "reject" | "supersede";

export type MemorySource = {
  provider: string;
  conversation_id: string;
  import_id: string;
  message_ids: string[];
  captured_at: string;
  source_content_sha256: string;
};

export type MemoryRecord = {
  id: string;
  workspace_id: string;
  kind: MemoryKind;
  title: string;
  content: string;
  summary: string;
  tags: string[];
  source: MemorySource;
  confidence: number;
  status: MemoryStatus;
  version: number;
  content_sha256: string;
  superseded_by_id: string | null;
  created_at: string;
  updated_at: string;
};

export type CreateMemoryRequest = {
  kind: MemoryKind;
  title: string;
  content: string;
  summary: string;
  tags?: string[];
  import_id: string;
  message_ids?: string[];
  confidence: number;
};

export type ReviewMemoryRequest = {
  expected_version: number;
  decision: MemoryReviewDecision;
  superseded_by_id: string | null;
};

export type ProjectMemoryEntry = {
  id: string;
  chat_id: string;
  type: "decision" | "fact" | "constraint" | "open_task" | "note";
  text: string;
  reason: string | null;
  source_message_id: string | null;
  status: "proposed" | "approved" | "rejected" | "deleted";
  created_at: string;
  approved_at: string | null;
};

export type ChatDetail = {
  chat: Chat;
  messages: ChatMessage[];
  memory: ProjectMemoryEntry[];
  resume_brief: string;
};


/** Qoin's six-dimensional attended market-state vector; not a text embedding. */
export type QoinVector = {
  values: number[];
  model_version: string;
};

export type QoinVectorSnapshot = {
  symbol: string;
  vector: QoinVector;
  features: Record<string, number>;
  action: string;
  reason: string;
  decision_vector: [number, number] | null;
  event_ts_ms: number | null;
};

export type MemoryEpoch = {
  id: string;
  workspace_id: string;
  epoch_number: number;
  published_at: string;
  source_event_sequence: number;
  indexed_memory_count: number;
  snapshot_digest: string;
};

export type HandoverMemory = {
  memory_id: string;
  source_kind: string;
  source_event_sequence: number;
  payload_json: string;
  attention_score: number;
};

export type HandoverContext = {
  session_id: string;
  epoch: MemoryEpoch;
  initial: HandoverMemory[];
  deferred_count: number;
  achieved_coverage: number;
  budget_exhausted: boolean;
};

export type ProgressiveMemoryContext = {
  session_id: string;
  memories: HandoverMemory[];
  remaining_candidates: number;
};

export type QoinMemoryIndexReceipt = {
  memory_id: string;
  symbol: string;
  model_version: string;
  memory_context_timestamp_ms: number;
  qoin_event_ts_ms: number;
  indexed: boolean;
};

export type QoinChatMessageIndexReceipt = {
  message_id: string;
  symbol: string;
  model_version: string;
  context_timestamp_ms: number;
  qoin_event_ts_ms: number;
  indexed: boolean;
};
