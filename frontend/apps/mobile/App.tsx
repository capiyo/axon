import { useEffect, useState } from "react";
import {
  ActivityIndicator,
  Image,
  Pressable,
  SafeAreaView,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View
} from "react-native";
import { StatusBar } from "expo-status-bar";
import { axonTheme } from "../../packages/theme/src";
import { auth } from "./firebase";

import type { Agent, Artifact, Health, Task, Workspace, Chat, ChatDetail, ChatMessage, ProjectMemoryEntry, QoinVectorSnapshot, HandoverContext, ProgressiveMemoryContext, QoinMemoryIndexReceipt, QoinChatMessageIndexReceipt, ExploreAgent, ExploreComment, ModerationReport, Notification, NotificationPage } from "../../packages/contracts/src";


const API_BASE_URL = process.env.EXPO_PUBLIC_AXON_API_URL ?? "http://127.0.0.1:8080";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const token = await auth?.currentUser?.getIdToken();
  const response = await fetch(`${API_BASE_URL}${path}`, {
    ...init,
    headers: { "Content-Type": "application/json", ...(token ? { Authorization: `Bearer ${token}` } : {}), ...init?.headers }
  });
  if (response.status === 204) return undefined as T;
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }));
    throw new Error(body.error ?? "Request failed");
  }
  return response.json() as Promise<T>;
}

function taskProgress(status: string): number {
  switch (status) {
    case "completed": return 100;
    case "accepted": return 90;
    case "review_required":
    case "submitted": return 80;
    case "in_progress": return 60;
    case "acknowledged": return 30;
    case "offered": return 10;
    case "changes_requested": return 55;
    default: return 0;
  }
}

function ActionButton({ label, onPress, disabled = false, secondary = false }: {
  label: string;
  onPress: () => void;
  disabled?: boolean;
  secondary?: boolean;
}) {
  return (
    <Pressable onPress={onPress} disabled={disabled} style={[styles.button, secondary && styles.secondaryButton, disabled && styles.disabled]}>
      <Text style={[styles.buttonText, secondary && styles.secondaryButtonText]}>{label}</Text>
    </Pressable>
  );
}

function WorkspaceApp({ user }: { user: import("firebase/auth").User }) {
  const [health, setHealth] = useState<Health | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [selected, setSelected] = useState<Workspace | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [chats, setChats] = useState<Chat[]>([]);
  const [activeTab, setActiveTab] = useState<"chats" | "explore">("chats");
  const [profileOpen, setProfileOpen] = useState(false);
  const [mobileChatOpen, setMobileChatOpen] = useState(false);
  const [exploreAgents, setExploreAgents] = useState<ExploreAgent[]>([]);
  const [exploreComments, setExploreComments] = useState<Record<string, ExploreComment[]>>({});
  const [exploreSort, setExploreSort] = useState<"recent" | "popular" | "discussed">("recent");
  const [exploreDrafts, setExploreDrafts] = useState<Record<string, string>>({});
  const [editingCommentId, setEditingCommentId] = useState<string | null>(null);
  const [editingCommentDraft, setEditingCommentDraft] = useState("");
  const [reportTarget, setReportTarget] = useState<{ listingId: string; commentId?: string } | null>(null);
  const [reportReason, setReportReason] = useState("");
  const [moderationReports, setModerationReports] = useState<Record<string, ModerationReport[]>>({});
  const [moderationOpenListing, setModerationOpenListing] = useState<string | null>(null);
  const [replyTarget, setReplyTarget] = useState<{ listingId: string; parentId: string } | null>(null);
  const [publishAgentId, setPublishAgentId] = useState("");
  const [publishDescription, setPublishDescription] = useState("");
  const [publishCategory, setPublishCategory] = useState("Productivity");
  const [notifications, setNotifications] = useState<Notification[]>([]);
  const [notificationUnread, setNotificationUnread] = useState(0);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const [activeChat, setActiveChat] = useState("");
  const [chatDetail, setChatDetail] = useState<ChatDetail | null>(null);
  const [chatTitle, setChatTitle] = useState("");
  const [messageDraft, setMessageDraft] = useState("");
  const [memoryDraft, setMemoryDraft] = useState("");
  const [memoryType, setMemoryType] = useState<ProjectMemoryEntry["type"]>("note");
  const [editingMemoryId, setEditingMemoryId] = useState("");
  const [editMemoryDraft, setEditMemoryDraft] = useState("");
  const [workspaceName, setWorkspaceName] = useState("");
  const [agentName, setAgentName] = useState("");
  const [provider, setProvider] = useState("claude");
  const [artifactTitle, setArtifactTitle] = useState("");
  const [artifactContent, setArtifactContent] = useState("");
  const [taskTitle, setTaskTitle] = useState("");
  const [taskObjective, setTaskObjective] = useState("");
  const [assignee, setAssignee] = useState("");
  const [selectedArtifactIds, setSelectedArtifactIds] = useState<string[]>([]);
  const [responseDrafts, setResponseDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [qoinSymbol, setQoinSymbol] = useState("BTCUSDT");
  const [qoinVector, setQoinVector] = useState<QoinVectorSnapshot | null>(null);
  const [handoverAgentId, setHandoverAgentId] = useState("");
  const [handover, setHandover] = useState<HandoverContext | null>(null);
  const [progressiveContext, setProgressiveContext] = useState<ProgressiveMemoryContext | null>(null);
  const [qoinNotice, setQoinNotice] = useState("");

  async function refreshHome() {
    const [status, items] = await Promise.all([
      request<Health>("/health"),
      request<Workspace[]>("/api/v1/workspaces")
    ]);
    setHealth(status);
    setWorkspaces(items);
    setSelected(current => current ?? items[0] ?? null);
  }

  async function refreshWorkspace(workspace: Workspace) {
    const [agentItems, artifactItems, taskItems] = await Promise.all([
      request<Agent[]>(`/api/v1/workspaces/${workspace.id}/agents`),
      request<Artifact[]>(`/api/v1/workspaces/${workspace.id}/artifacts`),
      request<Task[]>(`/api/v1/workspaces/${workspace.id}/tasks`)
    ]);
    setAgents(agentItems);
    setArtifacts(artifactItems);
    setTasks(taskItems);
    const chatItems = await request<Chat[]>(`/api/v1/workspaces/${workspace.id}/chats`);
    setChats(chatItems);
    setActiveChat(current => current && chatItems.some(chat => chat.id === current) ? current : chatItems[0]?.id ?? "");
    if (activeChat && !chatItems.some(chat => chat.id === activeChat)) setChatDetail(null);
    if (!agentItems.some(agent => agent.id === assignee)) setAssignee(agentItems[0]?.id ?? "");
    if (!agentItems.some(agent => agent.id === handoverAgentId)) setHandoverAgentId(agentItems[0]?.id ?? "");
  }

  useEffect(() => {
    refreshHome().catch(cause => setError(cause instanceof Error ? cause.message : "API unavailable"));
  }, []);

  async function refreshNotifications() {
    const page = await request<NotificationPage>("/api/v1/notifications?limit=30&offset=0");
    setNotifications(page.items);
    setNotificationUnread(page.unread_count);
  }

  function openNotification(item: Notification) {
    void perform(async () => {
      await request(`/api/v1/notifications/${item.id}/read`, { method: "POST" });
      setNotificationsOpen(false);
      if (item.entity_type === "chat" && item.workspace_id) {
        const workspace = workspaces.find(candidate => candidate.id === item.workspace_id);
        if (workspace) {
          setSelected(workspace);
          setActiveChat(item.entity_id);
          setActiveTab("chats");
          setMobileChatOpen(true);
        }
      } else if (item.entity_type === "task" && item.workspace_id) {
        const workspace = workspaces.find(candidate => candidate.id === item.workspace_id);
        if (workspace) setSelected(workspace);
        setProfileOpen(true);
      } else {
        setActiveTab("explore");
      }
      await refreshNotifications();
    });
  }

  function markAllNotificationsRead() {
    void perform(async () => {
      await request("/api/v1/notifications/read-all", { method: "POST" });
      await refreshNotifications();
    });
  }

  async function refreshExplore() {
    const listings = await request<ExploreAgent[]>(`/api/v1/explore/agents?sort=${exploreSort}&limit=30&offset=0`);
    setExploreAgents(listings);
    const entries = await Promise.all(listings.map(async listing => [listing.id, await request<ExploreComment[]>(`/api/v1/explore/agents/${listing.id}/comments?limit=50&offset=0`)] as const));
    setExploreComments(Object.fromEntries(entries));
  }

  function loadMoreExploreComments(listingId: string) {
    const current = exploreComments[listingId] ?? [];
    void perform(async () => {
      const next = await request<ExploreComment[]>(`/api/v1/explore/agents/${listingId}/comments?limit=50&offset=${current.length}`);
      setExploreComments(value => ({ ...value, [listingId]: [...(value[listingId] ?? []), ...next] }));
    });
  }

  function publishExploreAgent() {
    if (!selected || !publishAgentId || !publishDescription.trim() || !publishCategory.trim()) return;
    void perform(async () => {
      await request(`/api/v1/workspaces/${selected.id}/agents/${publishAgentId}/publish`, {
        method: "POST",
        body: JSON.stringify({ description: publishDescription.trim(), category: publishCategory.trim() })
      });
      setPublishDescription("");
      await refreshExplore();
    });
  }

  function toggleExploreAction(path: string) {
    void perform(async () => {
      await request(path, { method: "POST" });
      await refreshExplore();
    });
  }

  function submitExploreComment(listingId: string, parentId: string | null = null) {
    const body = exploreDrafts[listingId]?.trim() ?? "";
    if (!body) return;
    void perform(async () => {
      await request(`/api/v1/explore/agents/${listingId}/comments`, {
        method: "POST",
        body: JSON.stringify({ body, parent_comment_id: parentId })
      });
      setExploreDrafts(current => ({ ...current, [listingId]: "" }));
      setReplyTarget(null);
      await refreshExplore();
    });
  }

  function loadModerationReports(listingId: string) {
    void perform(async () => {
      const reports = await request<ModerationReport[]>(`/api/v1/explore/agents/${listingId}/reports?limit=50&offset=0`);
      setModerationReports(current => ({ ...current, [listingId]: reports }));
      setModerationOpenListing(listingId);
    });
  }

  function resolveModerationReport(listingId: string, reportId: string, action: "dismiss" | "hide_comment") {
    void perform(async () => {
      await request(`/api/v1/explore/reports/${reportId}/resolve`, { method: "POST", body: JSON.stringify({ action }) });
      const reports = await request<ModerationReport[]>(`/api/v1/explore/agents/${listingId}/reports?limit=50&offset=0`);
      setModerationReports(current => ({ ...current, [listingId]: reports }));
      await refreshExplore();
    });
  }

  function submitExploreReport() {
    if (!reportTarget || !reportReason.trim()) return;
    void perform(async () => {
      await request("/api/v1/explore/reports", { method: "POST", body: JSON.stringify({ listing_id: reportTarget.listingId, comment_id: reportTarget.commentId ?? null, reason: reportReason.trim() }) });
      setReportTarget(null);
      setReportReason("");
    });
  }

  function saveExploreComment(commentId: string) {
    if (!editingCommentDraft.trim()) return;
    void perform(async () => {
      await request(`/api/v1/explore/comments/${commentId}`, { method: "PATCH", body: JSON.stringify({ body: editingCommentDraft.trim() }) });
      setEditingCommentId(null);
      setEditingCommentDraft("");
      await refreshExplore();
    });
  }

  function deleteExploreComment(commentId: string) {
    void perform(async () => {
      await request(`/api/v1/explore/comments/${commentId}`, { method: "DELETE" });
      await refreshExplore();
    });
  }

  function unpublishExploreAgent(listing: ExploreAgent) {
    void perform(async () => {
      await request(`/api/v1/workspaces/${listing.workspace_id}/agents/${listing.agent_id}/publish`, { method: "DELETE" });
      await refreshExplore();
    });
  }

  function startExploreConversation(listing: ExploreAgent) {
    if (!selected) return;
    void perform(async () => {
      const chat = await request<Chat>(`/api/v1/workspaces/${selected.id}/chats`, {
        method: "POST",
        body: JSON.stringify({ title: listing.title })
      });
      setChats(current => [chat, ...current]);
      setActiveChat(chat.id);
      setActiveTab("chats");
      setMobileChatOpen(true);
    });
  }

  useEffect(() => {
    refreshNotifications().catch(() => undefined);
  }, []);

  useEffect(() => {
    if (activeTab === "explore") {
      refreshExplore().catch(cause => setError(cause instanceof Error ? cause.message : "Could not load Explore."));
    }
  }, [activeTab, exploreSort, selected?.id]);

  useEffect(() => {
    if (selected) {
      setQoinVector(null);
      setHandover(null);
      setProgressiveContext(null);
      setQoinNotice("");
      setSelectedArtifactIds([]);
      setResponseDrafts({});
      refreshWorkspace(selected).catch(cause => setError(cause instanceof Error ? cause.message : "Could not load workspace"));
    }
  }, [selected?.id]);

  useEffect(() => {
    if (!selected || !activeChat || !mobileChatOpen) { setChatDetail(null); return; }
    const chatId = activeChat;
    request(`/api/v1/workspaces/${selected.id}/chats/${chatId}/read`, { method: "POST" })
      .then(() => Promise.all([
        request<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${chatId}`),
        request<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`)
      ]))
      .then(([detail, items]) => { setChatDetail(detail); setChats(items); })
      .catch(cause => setError(cause instanceof Error ? cause.message : "Could not load chat"));
  }, [selected?.id, activeChat, mobileChatOpen]);

  function createChat() {
    if (!selected || !chatTitle.trim()) return;
    void perform(async () => {
      const chat = await request<Chat>(`/api/v1/workspaces/${selected.id}/chats`, {
        method: "POST", body: JSON.stringify({ title: chatTitle.trim() })
      });
      setChatTitle("");
      setChats(current => [chat, ...current]);
      setActiveChat(chat.id);
      setMobileChatOpen(true);
    });
  }

  function archiveChat() {
    if (!selected || !activeChat) return;
    void perform(async () => {
      await request(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/archive`, { method: "PATCH" });
      const remaining = await request<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`);
      setChats(remaining);
      setChatDetail(null);
      setActiveChat(remaining[0]?.id ?? "");
      setMobileChatOpen(false);
    });
  }

  function sendChatMessage() {
    if (!selected || !activeChat || !messageDraft.trim()) return;
    void perform(async () => {
      await request<ChatMessage>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/messages`, {
        method: "POST", body: JSON.stringify({ role: "user", content: messageDraft.trim() })
      });
      setMessageDraft("");
      const [detail, items] = await Promise.all([
        request<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`),
        request<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`)
      ]);
      setChatDetail(detail);
      setChats(items);
    });
  }

  function toggleChatLike(chatId: string) {
    if (!selected) return;
    void perform(async () => {
      await request(`/api/v1/workspaces/${selected.id}/chats/${chatId}/like`, { method: "POST" });
      setChats(await request<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`));
    });
  }

  function archiveChatById(chatId: string) {
    if (!selected) return;
    void perform(async () => {
      await request(`/api/v1/workspaces/${selected.id}/chats/${chatId}/archive`, { method: "PATCH" });
      const remaining = await request<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`);
      setChats(remaining);
      if (activeChat === chatId) {
        setMobileChatOpen(false);
        setActiveChat(remaining[0]?.id ?? "");
      }
    });
  }

  function proposeMemory() {
    if (!selected || !activeChat || !memoryDraft.trim()) return;
    void perform(async () => {
      await request<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory`, {
        method: "POST", body: JSON.stringify({ type: memoryType, text: memoryDraft.trim() })
      });
      setMemoryDraft("");
      setChatDetail(await request<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    });
  }

  function changeMemory(entry: ProjectMemoryEntry, action: "reject" | "delete") {
    if (!selected || !activeChat) return;
    void perform(async () => {
      await request<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory/${entry.id}/${action}`, { method: "POST" });
      setChatDetail(await request<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    });
  }

  function saveMemoryEdit(entry: ProjectMemoryEntry) {
    if (!selected || !activeChat || !editMemoryDraft.trim()) return;
    void perform(async () => {
      await request<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory/${entry.id}/edit`, {
        method: "POST", body: JSON.stringify({ text: editMemoryDraft.trim(), reason: entry.reason })
      });
      setEditingMemoryId("");
      setEditMemoryDraft("");
      setChatDetail(await request<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    });
  }

  function approveMemory(entry: ProjectMemoryEntry) {
    if (!selected || !activeChat) return;
    void perform(async () => {
      await request<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory/${entry.id}/approve`, { method: "POST" });
      setChatDetail(await request<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    });
  }


  async function loadQoinVector() {
    if (!selected || !qoinSymbol.trim()) return;
    setQoinNotice("");
    await perform(async () => {
      const snapshot = await request<QoinVectorSnapshot>(
        `/api/v1/workspaces/${selected.id}/memory/qoin-vector?symbol=${encodeURIComponent(qoinSymbol.trim())}`
      );
      setQoinVector(snapshot);
      setQoinNotice(`Loaded ${snapshot.symbol} · ${snapshot.vector.model_version}`);
      setHandover(null);
      setProgressiveContext(null);
    });
  }

  async function indexChatMessageWithQoin(message: ChatMessage) {
    if (!selected || !activeChat || !qoinSymbol.trim()) return;
    await perform(async () => {
      const receipt = await request<QoinChatMessageIndexReceipt>(
        `/api/v1/workspaces/${selected.id}/chats/${activeChat}/messages/${message.id}/qoin-index`,
        {
          method: "POST",
          body: JSON.stringify({
            symbol: qoinSymbol.trim(),
            tags: [message.role, "source:chat_message", `symbol:${qoinSymbol.trim().toUpperCase()}`],
            dependency_ids: [],
            evidence_score: 0.5
          })
        }
      );
      setQoinNotice(`Indexed chat message ${receipt.message_id.slice(0, 8)} using ${receipt.symbol} at ${new Date(receipt.qoin_event_ts_ms).toLocaleString()}; available after the next hourly epoch.`);
    });
  }

  async function indexMemoryWithQoin(entry: ProjectMemoryEntry) {
    if (!selected || !activeChat || !qoinSymbol.trim()) return;
    await perform(async () => {
      const receipt = await request<QoinMemoryIndexReceipt>(
        `/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory-index/${entry.id}/qoin`,
        {
          method: "POST",
          body: JSON.stringify({
            symbol: qoinSymbol.trim(),
            visibility: "workspace_shared",
            tags: [entry.type, `symbol:${qoinSymbol.trim().toUpperCase()}`],
            dependency_ids: [],
            outcome_score: 0.5,
            evidence_score: 0.5
          })
        }
      );
      setQoinNotice(`Indexed ${receipt.memory_id.slice(0, 8)} using ${receipt.symbol} at ${new Date(receipt.qoin_event_ts_ms).toLocaleString()}; available after the next hourly epoch.`);
    });
  }

  async function prepareAgentHandover() {
    if (!selected || !handoverAgentId || !qoinVector) return;
    await perform(async () => {
      const result = await request<HandoverContext>(
        `/api/v1/workspaces/${selected.id}/agents/${handoverAgentId}/memory-context`,
        {
          method: "POST",
          body: JSON.stringify({
            workspace_id: selected.id,
            agent_id: handoverAgentId,
            task_id: null,
            project_id: null,
            query: qoinVector.vector,
            heads: 2,
            tags: [`symbol:${qoinVector.symbol}`],
            dependency_ids: [],
            pareto: { initial_share: 0.2, target_contribution_coverage: 0.8 }
          })
        }
      );
      setHandover(result);
      setProgressiveContext(null);
    });
  }

  async function retrieveMoreContext() {
    if (!selected || !handoverAgentId || !handover || !qoinVector) return;
    await perform(async () => {
      const result = await request<ProgressiveMemoryContext>(
        `/api/v1/workspaces/${selected.id}/agents/${handoverAgentId}/memory-sessions/${handover.session_id}/retrieve`,
        {
          method: "POST",
          body: JSON.stringify({
            query: qoinVector.vector,
            heads: 2,
            tags: [`symbol:${qoinVector.symbol}`],
            dependency_ids: [],
            limit: 10
          })
        }
      );
      setProgressiveContext(result);
    });
  }

  async function perform(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The request failed");
    } finally {
      setBusy(false);
    }
  }

  function createWorkspace() {
    if (!workspaceName.trim()) return;
    void perform(async () => {
      const created = await request<Workspace>("/api/v1/workspaces", {
        method: "POST",
        body: JSON.stringify({ name: workspaceName.trim() })
      });
      setWorkspaceName("");
      await refreshHome();
      setSelected(created);
    });
  }

  function registerAgent() {
    if (!selected || !agentName.trim()) return;
    void perform(async () => {
      await request<Agent>(`/api/v1/workspaces/${selected.id}/agents`, {
        method: "POST",
        body: JSON.stringify({ display_name: agentName.trim(), provider })
      });
      setAgentName("");
      await refreshWorkspace(selected);
    });
  }

  function captureArtifact() {
    if (!selected || !artifactTitle.trim() || !artifactContent.trim()) return;
    void perform(async () => {
      await request<Artifact>(`/api/v1/workspaces/${selected.id}/artifacts`, {
        method: "POST",
        body: JSON.stringify({ title: artifactTitle.trim(), content: artifactContent.trim(), source_agent_id: null })
      });
      setArtifactTitle("");
      setArtifactContent("");
      await refreshWorkspace(selected);
    });
  }

  function createTask() {
    if (!selected || !assignee || !taskTitle.trim() || !taskObjective.trim()) return;
    void perform(async () => {
      await request<Task>(`/api/v1/workspaces/${selected.id}/tasks`, {
        method: "POST",
        body: JSON.stringify({
          title: taskTitle.trim(),
          objective: taskObjective.trim(),
          assignee_agent_id: assignee,
          source_chat_id: mobileChatOpen ? activeChat || null : null,
          context_artifact_ids: selectedArtifactIds
        })
      });
      setTaskTitle("");
      setTaskObjective("");
      setSelectedArtifactIds([]);
      await refreshWorkspace(selected);
    });
  }

  async function advanceTask(task: Task, status: string) {
    if (!selected) return;
    await perform(async () => {
      await request<Task>(`/api/v1/tasks/${task.id}/transitions`, {
        method: "POST",
        body: JSON.stringify({
          expected_version: task.version,
          status,
          note: `Moved to ${status.replaceAll("_", " ")} from the mobile workspace.`
        })
      });
      await refreshWorkspace(selected);
    });
  }

  async function submitTaskResponse(task: Task) {
    if (!selected) return;
    const content = responseDrafts[task.id]?.trim() ?? "";
    if (!content) return;
    await perform(async () => {
      await request(`/api/v1/tasks/${task.id}/submit`, {
        method: "POST",
        body: JSON.stringify({
          expected_version: task.version,
          title: `${task.title} — response`,
          content
        })
      });
      setResponseDrafts(current => ({ ...current, [task.id]: "" }));
      await refreshWorkspace(selected);
    });
  }

  const nextTaskStatus: Record<string, string> = {
    offered: "acknowledged",
    acknowledged: "in_progress",
    submitted: "review_required",
    review_required: "accepted",
    changes_requested: "in_progress",
    accepted: "completed"
  };

  return (
    <SafeAreaView style={styles.safe}>
      <StatusBar style="light" />
      <View style={styles.mobileRoot}>
        <View style={styles.mobileHeader}>
          <View style={styles.mobileBrandMark}><Text style={styles.mobileBrandMarkText}>A</Text></View>
          <View style={styles.mobileHeaderCopy}><Text style={styles.mobileBrand}>AXON</Text><Text style={styles.mobileWorkspace}>{selected?.name ?? "Your workspace"}</Text></View>
          <Pressable onPress={() => {setNotificationsOpen(true);void refreshNotifications();}} style={styles.mobileNotificationButton} accessibilityLabel="Notifications"><Text style={styles.mobileNotificationIcon}>🔔</Text>{notificationUnread > 0 ? <Text style={styles.mobileNotificationBadge}>{notificationUnread}</Text> : null}</Pressable>
          <Pressable onPress={() => setProfileOpen(true)} style={styles.mobileProfileButton} accessibilityLabel="Open profile">
            {user.photoURL ? <Image source={{uri:user.photoURL}} style={styles.mobileProfileImage}/> : <Text style={styles.mobileProfileInitial}>{(user.displayName || user.email || "A").slice(0,1).toUpperCase()}</Text>}
          </Pressable>
        </View>

        <ScrollView contentContainerStyle={styles.mobileContent} keyboardShouldPersistTaps="handled">
          {!selected ? (
            <View style={styles.panel}>
              <Text style={styles.panelTitle}>Create your workspace</Text>
              <Text style={styles.muted}>Start a private workspace for your agents and conversations.</Text>
              <TextInput value={workspaceName} onChangeText={setWorkspaceName} placeholder="Workspace name" style={styles.input}/>
              <ActionButton label="Create workspace" onPress={createWorkspace} disabled={!workspaceName.trim() || busy}/>
              {workspaces.map(workspace => <Pressable key={workspace.id} onPress={() => setSelected(workspace)} style={styles.workspaceRow}><Text style={styles.rowTitle}>{workspace.name}</Text></Pressable>)}
            </View>
          ) : profileOpen ? (
            <>
              <View style={styles.screenHeading}><Pressable onPress={() => setProfileOpen(false)}><Text style={styles.back}>← Back</Text></Pressable><Text style={styles.screenTitle}>Profile & workspace</Text><Text style={styles.description}>{user.email}</Text></View>
              <View style={styles.panel}>
                <View style={styles.profileIdentityMobile}>
                  {user.photoURL ? <Image source={{uri:user.photoURL}} style={styles.profileAvatarMobile}/> : <View style={styles.profileAvatarFallback}><Text style={styles.profileAvatarFallbackText}>{(user.displayName || user.email || "A").slice(0,1).toUpperCase()}</Text></View>}
                  <View style={{flex:1}}><Text style={styles.rowTitle}>{user.displayName || user.email || "Axon user"}</Text><Text style={styles.rowMeta}>{selected.name}</Text></View>
                </View>
                <View style={styles.mobileStatsGrid}>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{agents.length}</Text><Text style={styles.mobileStatLabel}>Agents</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{chats.length}</Text><Text style={styles.mobileStatLabel}>Chats</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{tasks.filter(task => task.status === "in_progress").length}</Text><Text style={styles.mobileStatLabel}>Running</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{tasks.filter(task => task.status === "completed").length}</Text><Text style={styles.mobileStatLabel}>Completed</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{tasks.filter(task => task.status === "offered" || task.status === "acknowledged").length}</Text><Text style={styles.mobileStatLabel}>Queued</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{artifacts.length}</Text><Text style={styles.mobileStatLabel}>Artifacts</Text></View>
                </View>
                <View style={styles.profileMetricsMobile}>
                  <Text style={styles.fieldLabel}>MEMORY & CONTEXT</Text>
                  <Text style={styles.rowMeta}>Selected chat approved memory entries · {(chatDetail?.memory.filter(entry=>entry.status==="approved").length ?? 0)}</Text>
                  <Text style={styles.rowMeta}>Messages across active chats · {chats.reduce((total,chat)=>total+chat.message_count,0)}</Text>
                  <Text style={styles.rowMeta}>Captured artifacts · {artifacts.length}</Text>
                  <Text style={styles.fieldLabel}>EXECUTION & HEALTH</Text>
                  <Text style={styles.rowMeta}>Awaiting review · {tasks.filter(task=>task.status==="review_required" || task.status==="submitted").length}</Text>
                  <Text style={styles.rowMeta}>Rejected / cancelled · {tasks.filter(task=>task.status==="rejected" || task.status==="cancelled").length}</Text>
                  <Text style={styles.rowMeta}>API · {health?.status === "ok" ? "Connected" : "Unavailable"}</Text>
                  <Text style={styles.rowMeta}>Storage · {health?.storage === "persistent" ? "Persistent" : health?.storage ?? "Unknown"}</Text>
                </View>
                <Text style={styles.fieldLabel}>AGENT REGISTRY</Text>
                <TextInput value={agentName} onChangeText={setAgentName} placeholder="Agent name" style={styles.input}/>
                <TextInput value={provider} onChangeText={setProvider} placeholder="Provider" style={styles.input}/>
                <ActionButton label="Register agent" onPress={registerAgent} disabled={!agentName.trim() || busy}/>
                {agents.map(agent => <View key={agent.id} style={styles.listRow}><Text style={styles.rowTitle}>{agent.display_name}</Text><Text style={styles.rowMeta}>{agent.provider} · manually registered</Text></View>)}
                <Text style={styles.fieldLabel}>PUBLISH TO EXPLORE</Text>
                {agents.map(agent => <Pressable key={agent.id} onPress={() => setPublishAgentId(agent.id)} style={[styles.agentChoice,publishAgentId===agent.id && styles.agentChoiceActive]}><Text style={styles.rowTitle}>{agent.display_name} · {agent.provider}</Text><Text style={styles.rowMeta}>{publishAgentId===agent.id ? "Selected for publishing" : "Select to publish"}</Text></Pressable>)}
                <TextInput value={publishCategory} onChangeText={setPublishCategory} placeholder="Category" style={styles.input} maxLength={80}/>
                <TextInput value={publishDescription} onChangeText={setPublishDescription} placeholder="Public agent description…" style={[styles.input,styles.multiline]} multiline maxLength={4000}/>
                <ActionButton label="Publish selected agent" onPress={publishExploreAgent} disabled={!publishAgentId || !publishDescription.trim() || !publishCategory.trim() || busy} secondary/>
                <Text style={styles.fieldLabel}>CAPTURED CONTEXT</Text>
                <TextInput value={artifactTitle} onChangeText={setArtifactTitle} placeholder="Artifact title" style={styles.input}/>
                <TextInput value={artifactContent} onChangeText={setArtifactContent} placeholder="Capture findings and evidence…" style={[styles.input,styles.multiline]} multiline/>
                <ActionButton label="Capture artifact" onPress={captureArtifact} disabled={!artifactTitle.trim() || !artifactContent.trim() || busy}/>
                {artifacts.map(artifact => <View key={artifact.id} style={styles.listRow}><Text style={styles.rowTitle}>{artifact.title}</Text><Text style={styles.rowMeta}>{artifact.content}</Text></View>)}
                <Text style={styles.fieldLabel}>QOIN × AXON MEMORY CONTEXT</Text>
                <TextInput value={qoinSymbol} onChangeText={value => {setQoinSymbol(value.toUpperCase());setQoinVector(null);setHandover(null);setProgressiveContext(null);}} placeholder="Market symbol (BTCUSDT)" autoCapitalize="characters" style={styles.input}/>
                <ActionButton label="Load latest Qoin vector" onPress={loadQoinVector} disabled={busy || !qoinSymbol.trim()} secondary/>
                {qoinVector ? <View style={styles.listRow}><Text style={styles.rowTitle}>{qoinVector.symbol} · {qoinVector.vector.model_version} · {qoinVector.action.toUpperCase()}</Text><Text style={styles.rowMeta}>{qoinVector.reason}</Text>{Object.entries(qoinVector.features).map(([key,value]) => <Text key={key} style={styles.rowMeta}>{key.replaceAll("_"," ")}: {value.toFixed(3)}</Text>)}</View> : <Text style={styles.muted}>Load a current market-state vector to prepare bounded historical context for an agent.</Text>}
                <Text style={styles.fieldLabel}>RECEIVING AGENT</Text>
                {agents.map(agent => <Pressable key={agent.id} onPress={() => {setHandoverAgentId(agent.id);setHandover(null);setProgressiveContext(null);}} style={[styles.agentChoice,handoverAgentId===agent.id && styles.agentChoiceActive]}><Text style={styles.rowTitle}>{agent.display_name} · {agent.provider}</Text></Pressable>)}
                <ActionButton label="Prepare first 20% context" onPress={prepareAgentHandover} disabled={busy || !qoinVector || !handoverAgentId || agents.length===0} secondary/>
                {handover ? <View style={styles.listRow}><Text style={styles.rowTitle}>Epoch {handover.epoch.epoch_number} · {handover.initial.length} initial memories</Text><Text style={styles.rowMeta}>{(handover.achieved_coverage*100).toFixed(1)}% contribution coverage · {handover.deferred_count} deferred</Text>{handover.initial.map(item => <View key={item.memory_id} style={styles.contextChoice}><Text style={styles.rowTitle}>{item.source_kind} · {(item.attention_score*100).toFixed(1)}%</Text><Text style={styles.rowMeta}>{item.payload_json}</Text></View>)}<ActionButton label="Retrieve next 10 historical memories" onPress={retrieveMoreContext} disabled={busy || handover.deferred_count===0} secondary/>{progressiveContext ? progressiveContext.memories.map(item => <View key={item.memory_id} style={styles.contextChoice}><Text style={styles.rowTitle}>{item.source_kind} · {(item.attention_score*100).toFixed(1)}%</Text><Text style={styles.rowMeta}>{item.payload_json}</Text></View>) : null}</View> : null}
                <Text style={styles.fieldLabel}>TASK HANDOFFS</Text>
                <TextInput value={taskTitle} onChangeText={setTaskTitle} placeholder="Task title" style={styles.input}/>
                <TextInput value={taskObjective} onChangeText={setTaskObjective} placeholder="Expected outcome and instructions…" style={[styles.input,styles.multiline]} multiline/>
                {agents.map(agent => <Pressable key={agent.id} onPress={() => setAssignee(agent.id)} style={[styles.agentChoice,assignee===agent.id && styles.agentChoiceActive]}><Text style={styles.rowTitle}>{agent.display_name} · {agent.provider}</Text></Pressable>)}
                <ActionButton label="Create handoff" onPress={createTask} disabled={!taskTitle.trim() || !taskObjective.trim() || !assignee || busy}/>
                {tasks.map(task => <View key={task.id} style={styles.listRow}><Text style={styles.rowTitle}>{task.title}</Text><Text style={styles.rowMeta}>{task.status.replaceAll("_"," ")} · {task.transitions.length} events</Text>{nextTaskStatus[task.status] ? <ActionButton label={`Advance to ${nextTaskStatus[task.status].replaceAll("_"," ")}`} onPress={() => advanceTask(task,nextTaskStatus[task.status])} disabled={busy} secondary/> : null}{task.status === "in_progress" ? <><TextInput value={responseDrafts[task.id] ?? ""} onChangeText={value => setResponseDrafts(current => ({...current,[task.id]:value}))} placeholder="Capture agent response…" style={[styles.input,styles.multiline]} multiline/><ActionButton label="Submit response" onPress={() => submitTaskResponse(task)} disabled={busy || !(responseDrafts[task.id] ?? "").trim()} secondary/></> : null}</View>)}
                <ActionButton label="Sign out" onPress={() => { if (auth) void signOut(auth); }} secondary/>
              </View>
            </>
          ) : activeTab === "explore" ? (
            <>
              <View style={styles.screenHeading}><Text style={styles.eyebrow}>PUBLIC DISCOVERY</Text><Text style={styles.screenTitle}>Explore agents</Text><Text style={styles.description}>Discover agents that their owners have deliberately published, and read the community discussion.</Text></View>
              <View style={styles.exploreSortRow}>{([{value:"recent",label:"Newest"},{value:"popular",label:"Most liked"},{value:"discussed",label:"Discussed"}] as const).map(option => <Pressable key={option.value} onPress={() => setExploreSort(option.value)} style={[styles.exploreSortChip,exploreSort===option.value && styles.agentChoiceActive]}><Text style={styles.rowTitle}>{option.label}</Text></Pressable>)}</View>
              {exploreAgents.map(listing => {
                const comments = exploreComments[listing.id] ?? [];
                const roots = comments.filter(comment => !comment.parent_comment_id);
                return <View key={listing.id} style={styles.exploreCardMobile}>
                  <View style={styles.exploreCardHeader}><View style={styles.exploreAvatar}><Text style={styles.exploreAvatarText}>{listing.title.slice(0,1).toUpperCase()}</Text></View><View style={{flex:1}}><Text style={styles.rowTitle}>{listing.title}</Text><Text style={styles.rowMeta}>{listing.category} · {listing.provider}</Text></View><Text style={styles.exploreStatus}>Published</Text></View>
                  <Text style={styles.exploreDescription}>{listing.description}</Text>
                  <View style={styles.exploreMeta}><Text style={styles.rowMeta}>{listing.like_count} likes · {listing.comment_count} comments</Text><Text style={styles.rowMeta}>{new Date(listing.updated_at).toLocaleDateString()}</Text></View>
                  <View style={styles.exploreActions}><Pressable onPress={() => toggleExploreAction(`/api/v1/explore/agents/${listing.id}/like`)}><Text style={styles.exploreActionText}>{listing.is_liked ? "♥ Liked" : "♡ Like"}</Text></Pressable><Pressable onPress={() => toggleExploreAction(`/api/v1/explore/agents/${listing.id}/follow`)}><Text style={styles.exploreActionText}>{listing.is_following ? "Following" : "Follow"}</Text></Pressable><Pressable onPress={() => startExploreConversation(listing)}><Text style={styles.exploreActionText}>Start workspace chat</Text></Pressable>{listing.is_owner ? <Pressable onPress={() => unpublishExploreAgent(listing)}><Text style={styles.exploreActionText}>Unpublish</Text></Pressable> : null}{listing.is_owner ? <Pressable onPress={() => {if(moderationOpenListing===listing.id){setModerationOpenListing(null);}else{loadModerationReports(listing.id);}}}><Text style={styles.exploreActionText}>Moderate reports</Text></Pressable> : null}<Pressable onPress={() => {setReportTarget({listingId:listing.id});setReportReason("");}}><Text style={styles.exploreActionText}>Report</Text></Pressable></View>
                  {moderationOpenListing===listing.id ? <View style={styles.exploreModerationMobile}><Text style={styles.fieldLabel}>PUBLISHER MODERATION</Text>{(moderationReports[listing.id] ?? []).map(report => <View key={report.id} style={styles.exploreModerationItemMobile}><Text style={styles.rowMeta}>{report.status} · {new Date(report.created_at).toLocaleString()}</Text><Text style={styles.exploreCommentBody}>{report.reason}</Text>{report.status==="pending" ? <View style={styles.exploreActions}><Pressable onPress={() => resolveModerationReport(listing.id,report.id,"dismiss")}><Text style={styles.exploreActionText}>Dismiss</Text></Pressable>{report.comment_id ? <Pressable onPress={() => resolveModerationReport(listing.id,report.id,"hide_comment")}><Text style={styles.exploreActionText}>Hide comment</Text></Pressable> : null}</View> : null}</View>)}{(moderationReports[listing.id] ?? []).length===0 ? <Text style={styles.muted}>No reports for this listing.</Text> : null}</View> : null}<View style={styles.exploreDiscussionMobile}><Text style={styles.fieldLabel}>COMMUNITY DISCUSSION</Text>{reportTarget?.listingId===listing.id ? <View style={styles.exploreReportForm}><TextInput value={reportReason} onChangeText={setReportReason} placeholder="Why are you reporting this content?" style={[styles.input,styles.multiline]} multiline/><ActionButton label="Submit report" onPress={submitExploreReport} disabled={!reportReason.trim() || busy} secondary/><Pressable onPress={() => setReportTarget(null)}><Text style={styles.exploreActionText}>Cancel report</Text></Pressable></View> : null}
                    {roots.map(comment => <View key={comment.id} style={styles.exploreCommentMobile}><View style={styles.exploreCommentAvatar}><Text style={styles.exploreAvatarText}>A</Text></View><View style={{flex:1}}><Text style={styles.exploreCommentAuthor}>{comment.author_label} · {new Date(comment.created_at).toLocaleString()}</Text>{editingCommentId===comment.id ? <><TextInput value={editingCommentDraft} onChangeText={setEditingCommentDraft} style={[styles.input,styles.multiline]} multiline/><View style={styles.exploreActions}><Pressable onPress={() => saveExploreComment(comment.id)}><Text style={styles.exploreActionText}>Save</Text></Pressable><Pressable onPress={() => setEditingCommentId(null)}><Text style={styles.exploreActionText}>Cancel</Text></Pressable></View></> : <Text style={styles.exploreCommentBody}>{comment.body}</Text>}<View style={styles.exploreActions}><Pressable onPress={() => toggleExploreAction(`/api/v1/explore/comments/${comment.id}/like`)}><Text style={styles.exploreActionText}>{comment.is_liked ? "♥" : "♡"} {comment.like_count}</Text></Pressable><Pressable onPress={() => setReplyTarget(replyTarget?.parentId===comment.id ? null : {listingId:listing.id,parentId:comment.id})}><Text style={styles.exploreActionText}>Reply · {comment.reply_count}</Text></Pressable><Pressable onPress={() => {setReportTarget({listingId:listing.id,commentId:comment.id});setReportReason("");}}><Text style={styles.exploreActionText}>Report</Text></Pressable>{comment.is_mine ? <><Pressable onPress={() => {setEditingCommentId(comment.id);setEditingCommentDraft(comment.body);}}><Text style={styles.exploreActionText}>Edit</Text></Pressable><Pressable onPress={() => deleteExploreComment(comment.id)}><Text style={styles.exploreActionText}>Delete</Text></Pressable></> : null}</View>
                      {comments.filter(reply => reply.parent_comment_id===comment.id).map(reply => <View key={reply.id} style={styles.exploreReplyMobile}><Text style={styles.exploreCommentAuthor}>{reply.author_label} · {new Date(reply.created_at).toLocaleString()}</Text>{editingCommentId===reply.id ? <><TextInput value={editingCommentDraft} onChangeText={setEditingCommentDraft} style={[styles.input,styles.multiline]} multiline/><View style={styles.exploreActions}><Pressable onPress={() => saveExploreComment(reply.id)}><Text style={styles.exploreActionText}>Save</Text></Pressable><Pressable onPress={() => setEditingCommentId(null)}><Text style={styles.exploreActionText}>Cancel</Text></Pressable></View></> : <Text style={styles.exploreCommentBody}>{reply.body}</Text>}<View style={styles.exploreActions}><Pressable onPress={() => toggleExploreAction(`/api/v1/explore/comments/${reply.id}/like`)}><Text style={styles.exploreActionText}>{reply.is_liked ? "♥" : "♡"} {reply.like_count}</Text></Pressable>{reply.is_mine ? <><Pressable onPress={() => {setEditingCommentId(reply.id);setEditingCommentDraft(reply.body);}}><Text style={styles.exploreActionText}>Edit</Text></Pressable><Pressable onPress={() => deleteExploreComment(reply.id)}><Text style={styles.exploreActionText}>Delete</Text></Pressable></> : null}</View></View>)}
                      {replyTarget?.listingId===listing.id && replyTarget.parentId===comment.id ? <View style={styles.exploreCommentInputRow}><TextInput value={exploreDrafts[listing.id] ?? ""} onChangeText={value => setExploreDrafts(current => ({...current,[listing.id]:value}))} placeholder="Write a reply…" style={[styles.input,styles.exploreCommentInput]}/><ActionButton label="Reply" onPress={() => submitExploreComment(listing.id,comment.id)} disabled={! (exploreDrafts[listing.id] ?? "").trim() || busy}/></View> : null}
                    </View></View>)}
                    {roots.length===0 ? <Text style={styles.muted}>No comments yet. Start the discussion.</Text> : null}
                    {comments.length >= 50 ? <Pressable onPress={() => loadMoreExploreComments(listing.id)}><Text style={styles.exploreActionText}>Load more comments</Text></Pressable> : null}<View style={styles.exploreCommentInputRow}><TextInput value={replyTarget?.listingId===listing.id ? "" : exploreDrafts[listing.id] ?? ""} onChangeText={value => setExploreDrafts(current => ({...current,[listing.id]:value}))} placeholder="Write a comment…" style={[styles.input,styles.exploreCommentInput]}/><ActionButton label="Post" onPress={() => submitExploreComment(listing.id)} disabled={!(exploreDrafts[listing.id] ?? "").trim() || busy}/></View>
                  </View>
                </View>;
              })}
              {exploreAgents.length===0 ? <View style={styles.panel}><Text style={styles.rowTitle}>No public agents yet</Text><Text style={styles.muted}>Publish an agent from Profile to make it discoverable.</Text></View> : null}
            </>
          ) : mobileChatOpen && chatDetail ? (
            <>
              <View style={styles.screenHeading}><Pressable onPress={() => setMobileChatOpen(false)}><Text style={styles.back}>← Chats</Text></Pressable><Text style={styles.screenTitle}>{chatDetail.chat.title}</Text><Text style={styles.description}>{chatDetail.messages.length} messages · Updated {new Date(chatDetail.chat.updated_at).toLocaleString()}</Text></View>
              <View style={styles.panel}>
                <View style={styles.chatAgentHeader}><View style={styles.exploreAvatar}><Text style={styles.exploreAvatarText}>A</Text></View><View style={{flex:1}}><Text style={styles.rowTitle}>{chatDetail.chat.title}</Text><Text style={styles.rowMeta}>Workspace conversation</Text></View><Pressable onPress={archiveChat}><Text style={styles.back}>Archive</Text></Pressable></View>
                <View style={styles.resumeBriefMobile}><Text style={styles.fieldLabel}>APPROVED MEMORY · RESUME BRIEF</Text><Text style={styles.rowMeta}>{chatDetail.resume_brief || "No approved context yet."}</Text></View>
                {chatDetail.messages.map(message => <View key={message.id} style={[styles.messageBubbleMobile,message.role==="user" && styles.userMessageBubble]}><Text style={styles.messageSenderMobile}>{message.role === "user" ? "You" : message.role === "assistant" ? (message.answered_by || "Agent") : "System"} · {new Date(message.created_at).toLocaleString()}</Text><Text style={styles.messageTextMobile}>{message.content}</Text><ActionButton label="Index with Qoin" onPress={() => indexChatMessageWithQoin(message)} disabled={busy} secondary/></View>)}
                {chatDetail.messages.length === 0 ? <Text style={styles.muted}>No messages yet. Send the first message to begin.</Text> : null}
                <TextInput value={messageDraft} onChangeText={setMessageDraft} placeholder="Write a message…" style={[styles.input,styles.multiline]} multiline/>
                <ActionButton label="Send message" onPress={sendChatMessage} disabled={!messageDraft.trim() || busy}/>
                <Text style={styles.fieldLabel}>PROJECT MEMORY</Text>
                <TextInput value={memoryDraft} onChangeText={setMemoryDraft} placeholder="Propose a decision, fact or constraint…" style={[styles.input,styles.multiline]} multiline maxLength={4000}/>
                <View style={styles.memoryTypeRow}>{(["decision","fact","constraint","open_task","note"] as const).map(kind => <Pressable key={kind} onPress={() => setMemoryType(kind)} style={[styles.memoryTypeChip,memoryType===kind && styles.agentChoiceActive]}><Text style={styles.rowTitle}>{kind.replaceAll("_"," ")}</Text></Pressable>)}</View>
                <ActionButton label="Propose memory" onPress={proposeMemory} disabled={!memoryDraft.trim() || busy} secondary/>
                {chatDetail.memory.map(entry => <View key={entry.id} style={styles.listRow}><Text style={styles.rowTitle}>{entry.type.replaceAll("_"," ")} · {entry.status}</Text><Text style={styles.rowMeta}>{entry.text}</Text>{entry.status==="proposed" ? <><ActionButton label="Approve" onPress={() => approveMemory(entry)} disabled={busy} secondary/><ActionButton label="Reject" onPress={() => changeMemory(entry,"reject")} disabled={busy} secondary/></> : null}<ActionButton label="Edit" onPress={() => {setEditingMemoryId(entry.id);setEditMemoryDraft(entry.text);}} disabled={busy} secondary/>{editingMemoryId===entry.id ? <><TextInput value={editMemoryDraft} onChangeText={setEditMemoryDraft} style={styles.input}/><ActionButton label="Save edit" onPress={() => saveMemoryEdit(entry)} disabled={busy || !editMemoryDraft.trim()}/></> : null}<ActionButton label="Delete" onPress={() => changeMemory(entry,"delete")} disabled={busy} secondary/></View>)}
              </View>
            </>
          ) : (
            <>
              <View style={styles.screenHeading}><Text style={styles.eyebrow}>PRIVATE WORKSPACE</Text><Text style={styles.screenTitle}>Chats</Text><Text style={styles.description}>Conversations, agent work and project context in one place.</Text></View>
              <View style={styles.newChatRow}><TextInput value={chatTitle} onChangeText={setChatTitle} placeholder="Start a new conversation…" style={[styles.input,styles.newChatInput]} maxLength={160}/><ActionButton label="＋" onPress={createChat} disabled={!chatTitle.trim() || busy}/></View>
              {chats.map(chat => { const last = chat.last_message; const linkedTasks = tasks.filter(task => task.source_chat_id === chat.id); const progress = linkedTasks.length ? Math.round(linkedTasks.reduce((sum,task)=>sum+taskProgress(task.status),0)/linkedTasks.length) : null; return <View key={chat.id} style={styles.chatCardMobile}>
                <Pressable onPress={() => {setActiveChat(chat.id);setMobileChatOpen(true);}}>
                  <View style={styles.chatCardMobileHeader}><View style={styles.chatCardMobileAvatar}><Text style={styles.chatCardMobileAvatarText}>{(last?.answered_by || chat.title || "A").slice(0,1).toUpperCase()}</Text></View><View style={{flex:1}}><Text style={styles.chatCardMobileTitle}>{chat.title}</Text><Text style={styles.chatCardMobileAgent}>{last?.answered_by || "Workspace conversation"}</Text></View><Text style={styles.chatCardMobileTime}>{new Date(chat.updated_at).toLocaleString()}</Text></View>
                  <Text style={styles.chatCardMobilePreview}>{last?.content || "No messages yet. Open this conversation to begin."}</Text>{progress !== null ? <View style={styles.chatCardMobileProgress}><View style={[styles.chatCardMobileProgressFill,{width: `${progress}%`}]}/></View> : null}
                  <Text style={styles.chatCardMobileOpen}>OPEN CONVERSATION →</Text>
                </Pressable>
                <View style={styles.chatCardMobileFooter}><Text style={styles.chatCardMobileMeta}>{`${chat.message_count} messages`} · {linkedTasks.length} tasks · {last?.role === "assistant" ? "Agent replied" : last?.role === "user" ? "Your message" : "New conversation"}</Text><View style={styles.chatCardMobileActions}>{chat.unread_count > 0 ? <Text style={styles.chatUnreadCount}>{chat.unread_count}</Text> : null}<Pressable onPress={() => toggleChatLike(chat.id)}><Text style={styles.chatCardMobileLike}>{chat.is_liked ? "♥" : "♡"}</Text></Pressable><Pressable onPress={() => archiveChatById(chat.id)}><Text style={styles.chatCardMobileArchive}>Archive</Text></Pressable></View></View>
              </View>; })}
              {chats.length === 0 ? <View style={styles.panel}><Text style={styles.rowTitle}>No conversations yet</Text><Text style={styles.muted}>Create your first chat above.</Text></View> : null}
            </>
          )}
        </ScrollView>

        {!profileOpen && <View style={styles.mobileBottomNav}><Pressable onPress={() => {setActiveTab("chats");setMobileChatOpen(false);}} style={[styles.mobileNavItem,activeTab==="chats" && styles.mobileNavItemActive]}><Text style={styles.mobileNavIcon}>◌</Text><Text style={styles.mobileNavText}>Chats</Text>{chats.reduce((total,chat)=>total+chat.unread_count,0)>0 ? <Text style={styles.mobileNavBadge}>{chats.reduce((total,chat)=>total+chat.unread_count,0)}</Text> : null}</Pressable><Pressable onPress={() => {setActiveTab("explore");setMobileChatOpen(false);}} style={[styles.mobileNavItem,activeTab==="explore" && styles.mobileNavItemActive]}><Text style={styles.mobileNavIcon}>⌕</Text><Text style={styles.mobileNavText}>Explore</Text></Pressable></View>}
        {notificationsOpen && <View style={styles.notificationsOverlay}><View style={styles.notificationsHeader}><Text style={styles.screenTitle}>Notifications</Text><View style={styles.notificationsHeaderActions}><Pressable onPress={markAllNotificationsRead}><Text style={styles.exploreActionText}>Mark all read</Text></Pressable><Pressable onPress={() => setNotificationsOpen(false)}><Text style={styles.exploreActionText}>Close</Text></Pressable></View></View><ScrollView contentContainerStyle={styles.notificationsList}>{notifications.map(item => <Pressable key={item.id} onPress={() => openNotification(item)} style={[styles.notificationItemMobile,Boolean(item.read_at) && styles.notificationReadMobile]}><Text style={styles.rowTitle}>{item.title}</Text><Text style={styles.rowMeta}>{item.body}</Text><Text style={styles.notificationTime}>{new Date(item.created_at).toLocaleString()}</Text></Pressable>)}{notifications.length===0 ? <Text style={styles.muted}>You are all caught up.</Text> : null}</ScrollView></View>}
        {error ? <View style={styles.mobileError}><Text style={styles.errorText}>{error}</Text></View> : null}
      </View>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safe: { flex: 1, backgroundColor: axonTheme.color.canvas },
  page: { padding: 22, paddingTop: 26, paddingBottom: 48 },
  brandRow: { flexDirection: "row", alignItems: "center", marginBottom: 42 },
  logo: { width: 38, height: 38, borderRadius: 13, backgroundColor: axonTheme.color.accent, alignItems: "center", justifyContent: "center" },
  logoText: { color: axonTheme.color.accentText, fontSize: 21, fontWeight: "800" },
  brand: { marginLeft: 11, color: axonTheme.color.text, fontSize: 12, fontWeight: "800", letterSpacing: 2 },
  version: { marginLeft: "auto", backgroundColor: axonTheme.color.surfaceRaised, borderRadius: 20, paddingHorizontal: 10, paddingVertical: 5 },
  versionText: { color: axonTheme.color.accent, fontSize: 10, fontWeight: "700" },
  eyebrow: { color: axonTheme.color.accent, fontSize: 10, letterSpacing: 1.5, fontWeight: "700" },
  title: { color: axonTheme.color.text, fontSize: 34, fontWeight: "800", letterSpacing: -1.2, marginTop: 12 },
  description: { color: axonTheme.color.textMuted, fontSize: 14, lineHeight: 22, marginTop: 12, marginBottom: 22 },
  panel: { backgroundColor: axonTheme.color.surface, borderRadius: 17, padding: 17, marginTop: 14, borderWidth: 1, borderColor: axonTheme.color.border },
  panelTitle: { color: axonTheme.color.text, fontSize: 13, fontWeight: "800", marginBottom: 13 },
  statusRow: { flexDirection: "row", alignItems: "center", gap: 9, marginBottom: 8 },
  dot: { width: 8, height: 8, borderRadius: 4, backgroundColor: axonTheme.color.warning },
  online: { backgroundColor: axonTheme.color.success },
  statusText: { color: axonTheme.color.text, fontSize: 12, fontWeight: "700", flex: 1 },
  input: { borderWidth: 1, borderColor: axonTheme.color.border, borderRadius: 10, paddingHorizontal: 12, paddingVertical: 11, color: axonTheme.color.text, fontSize: 12, marginBottom: 9, backgroundColor: axonTheme.color.surfaceInput },
  multiline: { minHeight: 86, textAlignVertical: "top" },
  button: { borderRadius: 10, backgroundColor: axonTheme.color.accent, paddingVertical: 12, paddingHorizontal: 14, alignItems: "center", marginVertical: 5 },
  buttonText: { color: axonTheme.color.accentText, fontSize: 11, fontWeight: "800" },
  secondaryButton: { backgroundColor: axonTheme.color.surfaceRaised },
  secondaryButtonText: { color: axonTheme.color.accent },
  disabled: { opacity: 0.45 },
  listRow: { borderTopWidth: 1, borderTopColor: axonTheme.color.border, paddingVertical: 11, marginTop: 7 },
  contextChoice: { paddingVertical: 7, paddingHorizontal: 9, borderRadius: 9, backgroundColor: axonTheme.color.surfaceRaised, marginBottom: 5 },
  responseNote: { color: axonTheme.color.accent, fontSize: 10, marginTop: 8 },
  rowTitle: { color: axonTheme.color.text, fontSize: 12, fontWeight: "700" },
  rowMeta: { color: axonTheme.color.textMuted, fontSize: 10, lineHeight: 15, marginTop: 4 },
  muted: { color: axonTheme.color.textSubtle, fontSize: 11, lineHeight: 17, marginTop: 10 },
  fieldLabel: { color: axonTheme.color.textMuted, fontSize: 9, fontWeight: "800", letterSpacing: 1, marginBottom: 8 },
  agentChoice: { borderWidth: 1, borderColor: axonTheme.color.border, borderRadius: 10, padding: 10, marginBottom: 7 },
  agentChoiceActive: { borderColor: axonTheme.color.accent, backgroundColor: axonTheme.color.surfaceRaised },
  workspaceRow: { flexDirection: "row", alignItems: "center", paddingVertical: 12, borderTopWidth: 1, borderTopColor: axonTheme.color.border, marginTop: 8 },
  workspaceGlyph: { width: 34, height: 34, borderRadius: 12, backgroundColor: axonTheme.color.surfaceRaised, alignItems: "center", justifyContent: "center" },
  glyphText: { color: axonTheme.color.accent, fontSize: 13, fontWeight: "800" },
  workspaceCopy: { flex: 1, marginLeft: 11 },
  chevron: { color: axonTheme.color.textSubtle, fontSize: 24 },
  back: { color: axonTheme.color.accent, fontSize: 12, marginBottom: 20 },
  error: { backgroundColor: "#43292d", borderRadius: 10, padding: 12, marginTop: 14 },
  errorText: { color: axonTheme.color.danger, fontSize: 11 },
  footer: { color: axonTheme.color.textSubtle, fontSize: 9, letterSpacing: 1.1, marginTop: 34, textAlign: "center" },
  warning: { color: axonTheme.color.warning, fontSize: 10, lineHeight: 15, marginTop: 10, textAlign: "center" },

  mobileRoot: { flex:  1, backgroundColor: "#0E1612" },
  mobileHeader: { minHeight: 64, paddingHorizontal: 16, flexDirection: "row", alignItems: "center", gap: 11, backgroundColor: "#0E1612" },
  mobileBrandMark: { width: 38, height: 38, borderRadius: 19, backgroundColor: "#28583D", alignItems: "center", justifyContent: "center" },
  mobileBrandMarkText: { color: "#E7F6EB", fontSize: 18, fontWeight: "900" },
  mobileHeaderCopy: { flex: 1, gap: 2 },
  mobileBrand: { color: "#E7F6EB", fontSize: 11, fontWeight: "900", letterSpacing: 2 },
  mobileWorkspace: { color: "#9AAFA1", fontSize: 10 },
  mobileProfileButton: { width: 38, height: 38, borderRadius: 19, backgroundColor: "#28583D", alignItems: "center", justifyContent: "center", overflow: "hidden" },
  mobileNotificationButton: { width: 36, height: 36, borderRadius: 18, backgroundColor: "#17271D", alignItems: "center", justifyContent: "center" },
  mobileNotificationIcon: { color: "#9DE4B0", fontSize: 17 },
  mobileNotificationBadge: { position: "absolute", top: -4, right: -4, minWidth: 17, height: 17, borderRadius: 9, paddingHorizontal: 4, backgroundColor: "#28583D", color: "#E7F6EB", fontSize: 8, fontWeight: "900", textAlign: "center", textAlignVertical: "center" },
  mobileProfileImage: { width: 38, height: 38, borderRadius: 19 },
  mobileProfileInitial: { color: "#E7F6EB", fontSize: 14, fontWeight: "800" },
  mobileContent: { paddingHorizontal: 13, paddingTop: 10, paddingBottom: 112 },
  screenHeading: { marginBottom: 15, gap: 6 },
  screenTitle: { color: "#E7F6EB", fontSize: 25, fontWeight: "800", letterSpacing: -0.6 },
  profileIdentityMobile: { flexDirection: "row", alignItems: "center", gap: 12, marginBottom: 18 },
  profileMetricsMobile: { gap: 8, paddingVertical: 14, marginBottom: 14, borderTopWidth: 1, borderTopColor: "#294034", borderBottomWidth: 1, borderBottomColor: "#294034" },
  profileAvatarMobile: { width: 48, height: 48, borderRadius: 24 },
  profileAvatarFallback: { width: 48, height: 48, borderRadius: 24, backgroundColor: "#28583D", alignItems: "center", justifyContent: "center" },
  profileAvatarFallbackText: { color: "#E7F6EB", fontSize: 18, fontWeight: "800" },
  mobileStatsGrid: { flexDirection: "row", flexWrap: "wrap", gap: 8, marginBottom: 20 },
  mobileStat: { width: "31%", minHeight: 65, borderRadius: 12, padding: 10, backgroundColor: "#17271D", justifyContent: "center", gap: 3 },
  mobileStatValue: { color: "#E7F6EB", fontSize: 19, fontWeight: "800" },
  mobileStatLabel: { color: "#9AAFA1", fontSize: 9 },
  chatCardMobile: { width: "100%", padding: 16, borderRadius: 17, backgroundColor: "#14231B", marginBottom: 12, gap: 13 },
  chatCardMobileHeader: { flexDirection: "row", alignItems: "center", gap: 11 },
  chatCardMobileAvatar: { width: 46, height: 46, borderRadius: 23, backgroundColor: "#28583D", alignItems: "center", justifyContent: "center" },
  chatCardMobileAvatarText: { color: "#E7F6EB", fontSize: 15, fontWeight: "800" },
  chatCardMobileTitle: { color: "#E7F6EB", fontSize: 13, fontWeight: "800" },
  chatCardMobileAgent: { color: "#9AAFA1", fontSize: 10, marginTop: 4 },
  chatCardMobileTime: { color: "#82998A", fontSize: 9, maxWidth: 86, textAlign: "right" },
  chatCardMobilePreview: { color: "#E7F6EB", fontSize: 12, lineHeight: 19 },
  chatCardMobileProgress: { width: "100%", height: 4, overflow: "hidden", borderRadius: 999, backgroundColor: "#294A35" },
  chatCardMobileProgressFill: { height: 4, borderRadius: 999, backgroundColor: "#9DE4B0" },
  chatCardMobileFooter: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: 8 },
  chatCardMobileActions: { flexDirection: "row", alignItems: "center", gap: 13 },
  chatUnreadCount: { minWidth: 19, height: 19, overflow: "hidden", borderRadius: 10, textAlign: "center", textAlignVertical: "center", paddingHorizontal: 5, backgroundColor: "#28583D", color: "#E7F6EB", fontSize: 9, fontWeight: "900" },
  chatCardMobileLike: { color: "#9DE4B0", fontSize: 16, fontWeight: "800" },
  chatCardMobileArchive: { color: "#9AAFA1", fontSize: 9, fontWeight: "700" },
  chatCardMobileMeta: { color: "#9AAFA1", fontSize: 9, flex: 1 },
  chatCardMobileOpen: { color: "#9DE4B0", fontSize: 9, fontWeight: "800" },
  mobileBottomNav: { position: "absolute", bottom: 12, alignSelf: "center", width: 258, borderRadius: 28, backgroundColor: "#1C3024", padding: 7, flexDirection: "row", justifyContent: "space-around", gap: 8, elevation: 12 },
  mobileNavItem: { flex: 1, borderRadius: 22, paddingVertical: 9, flexDirection: "row", justifyContent: "center", alignItems: "center", gap: 7 },
  mobileNavItemActive: { backgroundColor: "#244B33" },
  mobileNavIcon: { color: "#9AAFA1", fontSize: 17 },
  mobileNavText: { color: "#9AAFA1", fontSize: 11, fontWeight: "700" },
  mobileNavBadge: { minWidth: 18, height: 18, borderRadius: 9, paddingHorizontal: 5, backgroundColor: "#28583D", color: "#E7F6EB", fontSize: 9, fontWeight: "900", textAlign: "center", textAlignVertical: "center" },
  mobileError: { marginHorizontal: 14, marginBottom: 72, padding: 10, borderRadius: 10, backgroundColor: "#43292d" },
  notificationsOverlay: { position: "absolute", zIndex: 45, top: 64, left: 10, right: 10, bottom: 78, padding: 14, borderRadius: 18, backgroundColor: "#14231B", elevation: 16 },
  notificationsHeader: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8, paddingBottom: 12 },
  notificationsHeaderActions: { flexDirection: "row", alignItems: "center", gap: 12 },
  notificationsList: { gap: 8, paddingBottom: 20 },
  notificationItemMobile: { padding: 12, borderRadius: 12, backgroundColor: "#17271D", gap: 5 },
  notificationReadMobile: { opacity: 0.65 },
  notificationTime: { color: "#82998A", fontSize: 9 },
  exploreCardMobile: { width: "100%", padding: 16, borderRadius: 17, backgroundColor: "#14231B", marginBottom: 12, gap: 12 },
  exploreCardHeader: { flexDirection: "row", alignItems: "center", gap: 11 },
  exploreAvatar: { width: 46, height: 46, borderRadius: 23, backgroundColor: "#28583D", alignItems: "center", justifyContent: "center" },
  exploreAvatarText: { color: "#E7F6EB", fontSize: 15, fontWeight: "800" },
  exploreStatus: { color: "#9DE4B0", fontSize: 9, fontWeight: "700" },
  exploreDescription: { color: "#E7F6EB", fontSize: 12, lineHeight: 19 },
  exploreMeta: { flexDirection: "row", justifyContent: "space-between", gap: 8 },
  chatAgentHeader: { flexDirection: "row", alignItems: "center", gap: 10, marginBottom: 12 },
  resumeBriefMobile: { padding: 12, borderRadius: 12, backgroundColor: "#17271D", marginBottom: 12 },
  messageBubbleMobile: { padding: 13, borderRadius: 14, backgroundColor: "#17271D", marginBottom: 10, gap: 7 },
  userMessageBubble: { backgroundColor: "#244B33", marginLeft: 18 },
  messageSenderMobile: { color: "#9DE4B0", fontSize: 9, fontWeight: "700" },
  messageTextMobile: { color: "#E7F6EB", fontSize: 12, lineHeight: 19 },
  memoryTypeRow: { flexDirection: "row", flexWrap: "wrap", gap: 7, marginBottom: 8 },
  memoryTypeChip: { paddingHorizontal: 9, paddingVertical: 7, borderRadius: 18, backgroundColor: "#17271D" },
  newChatRow: { flexDirection: "row", alignItems: "center", gap: 8, marginBottom: 13 },
  newChatInput: { flex: 1, marginBottom: 0 },

  exploreSortRow: { flexDirection: "row", flexWrap: "wrap", gap: 7, marginBottom: 12 },
  exploreSortChip: { paddingHorizontal: 12, paddingVertical: 8, borderRadius: 18, backgroundColor: "#17271D" },
  exploreActions: { flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 15, marginTop: 9 },
  exploreActionText: { color: "#9DE4B0", fontSize: 10, fontWeight: "700" },
  exploreDiscussionMobile: { borderTopWidth: 1, borderTopColor: "#294034", paddingTop: 12, marginTop: 4, gap: 12 },
  exploreModerationMobile: { padding: 11, borderRadius: 12, backgroundColor: "#17271D", gap: 9 },
  exploreModerationItemMobile: { gap: 5, paddingVertical: 8, borderTopWidth: 1, borderTopColor: "#294034" },
  exploreReportForm: { padding: 10, borderRadius: 12, backgroundColor: "#17271D", gap: 8 },
  exploreCommentMobile: { flexDirection: "row", alignItems: "flex-start", gap: 9 },
  exploreCommentAvatar: { width: 30, height: 30, borderRadius: 15, backgroundColor: "#28583D", alignItems: "center", justifyContent: "center" },
  exploreCommentAuthor: { color: "#9AAFA1", fontSize: 9, fontWeight: "700" },
  exploreCommentBody: { color: "#E7F6EB", fontSize: 11, lineHeight: 17, marginTop: 4 },
  exploreReplyMobile: { marginTop: 10, marginLeft: 10, paddingLeft: 10, borderLeftWidth: 2, borderLeftColor: "#244B33" },
  exploreCommentInputRow: { flexDirection: "row", alignItems: "center", gap: 7, marginTop: 8 },
  exploreCommentInput: { flex: 1, marginBottom: 0 },
});


function AuthGate() {
  const [user, setUser] = useState<import("firebase/auth").User | null>(null);
  const [ready, setReady] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [createAccount, setCreateAccount] = useState(false);
  const [authError, setAuthError] = useState("");
  const [working, setWorking] = useState(false);

  useEffect(() => {
    if (!auth) { setReady(true); return; }
    return onAuthStateChanged(auth, current => { setUser(current); setReady(true); });
  }, []);

  async function submit() {
    if (!auth) return;
    setWorking(true);
    setAuthError("");
    try {
      if (createAccount) await createUserWithEmailAndPassword(auth, email.trim(), password);
      else await signInWithEmailAndPassword(auth, email.trim(), password);
    } catch (cause) {
      setAuthError(cause instanceof Error ? cause.message : "Authentication failed.");
    } finally { setWorking(false); }
  }

  if (!ready) return <SafeAreaView style={styles.safe}><View style={styles.page}><ActivityIndicator/><Text style={styles.muted}>Restoring your secure session…</Text></View></SafeAreaView>;
  if (!auth) return <SafeAreaView style={styles.safe}><View style={styles.page}><Text style={styles.title}>Firebase configuration required</Text><Text style={styles.description}>Set the EXPO_PUBLIC_FIREBASE_* variables and restart the app.</Text></View></SafeAreaView>;
  if (user) return <WorkspaceApp user={user}/>;
  return <SafeAreaView style={styles.safe}><ScrollView contentContainerStyle={styles.page} keyboardShouldPersistTaps="handled"><View style={styles.brandRow}><View style={styles.logo}><Text style={styles.logoText}>A</Text></View><Text style={styles.brand}>AXON</Text></View><Text style={styles.eyebrow}>SECURE ACCESS</Text><Text style={styles.title}>{createAccount ? "Create your account" : "Welcome back"}</Text><Text style={styles.description}>Sign in to access your personal workspace and its data.</Text><TextInput value={email} onChangeText={setEmail} placeholder="Email address" keyboardType="email-address" autoCapitalize="none" autoComplete="email" style={styles.input}/><TextInput value={password} onChangeText={setPassword} placeholder="Password" secureTextEntry autoCapitalize="none" autoComplete={createAccount ? "new-password" : "password"} style={styles.input}/><ActionButton label={working ? "Please wait…" : createAccount ? "Create account" : "Sign in"} onPress={()=>void submit()} disabled={working || !email.trim() || password.length<6}/>{authError ? <Text style={styles.errorText}>{authError}</Text> : null}<Text onPress={()=>setCreateAccount(value=>!value)} style={styles.back}>{createAccount ? "Already registered? Sign in" : "New to Axon? Create an account"}</Text></ScrollView></SafeAreaView>;
}

import { onAuthStateChanged, signInWithEmailAndPassword, createUserWithEmailAndPassword, signOut } from "firebase/auth";

export default function App() { return <AuthGate />; }
