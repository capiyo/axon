import { useEffect, useState } from "react";
import WorkspaceView from "../views/WorkspaceView";
import HomeScreen from "./HomeScreen";
import SessionPanel from "./ChatsScreen";
import ExploreScreen from "./ExploreScreen";
import NotificationsModal from "../modals/NotificationsModal";
import ProfileModal from "../modals/ProfileModal";
import SessionCard from "../views/SessionCard";
import {
  ActivityIndicator,
  Image,
  Pressable,
  Platform,
  SafeAreaView,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View
} from "react-native";
import { StatusBar } from "expo-status-bar";
import { axonTheme } from "../../../packages/theme/src";
import AsyncStorage from "@react-native-async-storage/async-storage";
type GoogleUser = { uid: string; displayName: string | null; email: string | null; photoURL: string | null };
let googleIdToken: string | null = null;
function parseGoogleUser(token: string): GoogleUser {
  const payload = token.split(".")[1]?.replace(/-/g, "+").replace(/_/g, "/");
  if (!payload) throw new Error("Google returned an invalid identity token.");
  const claims = JSON.parse(globalThis.atob(payload)) as { sub: string; name?: string; email?: string; picture?: string; exp: number };
  if (!claims.sub || claims.exp * 1000 <= Date.now()) throw new Error("Google identity token has expired. Please sign in again.");
  return { uid: claims.sub, displayName: claims.name ?? null, email: claims.email ?? null, photoURL: claims.picture ?? null };
}
import { useIdTokenAuthRequest } from "expo-auth-session/providers/google";
import * as WebBrowser from "expo-web-browser";

import type { Agent, Artifact, Health, Task, Workspace, Chat, ChatDetail, ChatMessage, ProjectMemoryEntry, QoinVectorSnapshot, HandoverContext, ProgressiveMemoryContext, QoinMemoryIndexReceipt, QoinChatMessageIndexReceipt, ExploreAgent, ExploreComment, ModerationReport, Notification, NotificationPage } from "../../../packages/contracts/src";


WebBrowser.maybeCompleteAuthSession();

const API_BASE_URL = process.env.EXPO_PUBLIC_AXON_API_URL ?? "http://127.0.0.1:8080";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const token = googleIdToken;
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

function WorkspaceApp({ user, onSignOut }: { user: GoogleUser; onSignOut: () => void }) {
  const [health, setHealth] = useState<Health | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [selected, setSelected] = useState<Workspace | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [chats, setChats] = useState<Chat[]>([]);
  const [activeTab, setActiveTab] = useState<"chats" | "explore">("chats");
  const [quickChatOpen, setQuickChatOpen] = useState(false);
  const [profileOpen, setProfileOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [mobileChatOpen, setMobileChatOpen] = useState(false);
  const [exploreAgents, setExploreAgents] = useState<ExploreAgent[]>([]);
  const [selectedExploreAgent, setSelectedExploreAgent] = useState<ExploreAgent | null>(null);
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

  function openExploreAgent(listing: ExploreAgent) {
    void perform(async () => {
      const detail = await request<ExploreAgent>(`/api/v1/explore/agents/${listing.id}`);
      setSelectedExploreAgent(detail);
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
      .catch(cause => setError(cause instanceof Error ? cause.message : "Could not load session"));
  }, [selected?.id, activeChat, mobileChatOpen]);

  function createChat() {
    if (!selected || !chatTitle.trim()) return;
    void perform(async () => {
      const chat = await request<Chat>(`/api/v1/workspaces/${selected.id}/chats`, {
        method: "POST", body: JSON.stringify({ title: chatTitle.trim() })
      });
      setChatTitle("");
      setQuickChatOpen(false);
      setActiveTab("chats");
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
      setQoinNotice(`Indexed session turn ${receipt.message_id.slice(0, 8)} using ${receipt.symbol} at ${new Date(receipt.qoin_event_ts_ms).toLocaleString()}; available after the next hourly epoch.`);
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
      await request<Task>(`/api/v1/tasks/${task.id}/transition`, {
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

  async function executeTask(task: Task) {
    if (!selected) return;
    await perform(async () => {
      const memoryRequest = qoinVector
        ? {
            memory_query: qoinVector.vector,
            heads: 2,
            tags: [`symbol:${qoinVector.symbol}`],
            progressive_limit: 10
          }
        : {};
      await request<{ task: Task; artifact: Artifact }>(`/api/v1/tasks/${task.id}/execute`, {
        method: "POST",
        body: JSON.stringify(memoryRequest)
      });
      await refreshWorkspace(selected);
      if (task.source_chat_id && activeChat === task.source_chat_id && mobileChatOpen) {
        setChatDetail(await request<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
      }
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

  const profileContent = (
<>
              <View style={styles.screenHeading}><Pressable onPress={() => setProfileOpen(false)}><Text style={styles.back}>← Back</Text></Pressable><Text style={styles.screenTitle}>Profile & workspace</Text><Text style={styles.description}>{user.email}</Text></View>
              <View style={styles.panel}>
                <View style={styles.profileIdentityMobile}>
                  {user.photoURL ? <Image source={{uri:user.photoURL}} style={styles.profileAvatarMobile}/> : <View style={styles.profileAvatarFallback}><Text style={styles.profileAvatarFallbackText}>{(user.displayName || user.email || "A").slice(0,1).toUpperCase()}</Text></View>}
                  <View style={{flex:1}}><Text style={styles.rowTitle}>{user.displayName || user.email || "Axon user"}</Text><Text style={styles.rowMeta}>{selected?.name ?? "Personal workspace"}</Text></View>
                </View>
                <View style={styles.mobileStatsGrid}>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{agents.length}</Text><Text style={styles.mobileStatLabel}>Agents</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{chats.length}</Text><Text style={styles.mobileStatLabel}>Sessions</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{tasks.filter(task => task.status === "in_progress").length}</Text><Text style={styles.mobileStatLabel}>Running</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{tasks.filter(task => task.status === "completed").length}</Text><Text style={styles.mobileStatLabel}>Completed</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{tasks.filter(task => task.status === "offered" || task.status === "acknowledged").length}</Text><Text style={styles.mobileStatLabel}>Queued</Text></View>
                  <View style={styles.mobileStat}><Text style={styles.mobileStatValue}>{artifacts.length}</Text><Text style={styles.mobileStatLabel}>Artifacts</Text></View>
                </View>
                <View style={styles.profileMetricsMobile}>
                  <Text style={styles.fieldLabel}>MEMORY & CONTEXT</Text>
                  <Text style={styles.rowMeta}>Selected session approved memory entries · {(chatDetail?.memory.filter(entry=>entry.status==="approved").length ?? 0)}</Text>
                  <Text style={styles.rowMeta}>Session turns across active sessions · {chats.reduce((total,chat)=>total+chat.message_count,0)}</Text>
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
                {tasks.map(task => <View key={task.id} style={styles.listRow}><Text style={styles.rowTitle}>{task.title}</Text><Text style={styles.rowMeta}>{task.status.replaceAll("_"," ")} · {task.transitions.length} events</Text>{["offered","acknowledged","in_progress","changes_requested"].includes(task.status) ? <ActionButton label="Run assigned agent" onPress={() => void executeTask(task)} disabled={busy}/> : null}{nextTaskStatus[task.status] ? <ActionButton label={`Advance to ${nextTaskStatus[task.status].replaceAll("_"," ")}`} onPress={() => advanceTask(task,nextTaskStatus[task.status])} disabled={busy} secondary/> : null}{task.status === "in_progress" ? <><TextInput value={responseDrafts[task.id] ?? ""} onChangeText={value => setResponseDrafts(current => ({...current,[task.id]:value}))} placeholder="Capture agent response…" style={[styles.input,styles.multiline]} multiline/><ActionButton label="Submit response" onPress={() => submitTaskResponse(task)} disabled={busy || !(responseDrafts[task.id] ?? "").trim()} secondary/></> : null}</View>)}
                <ActionButton label="Sign out" onPress={() => { onSignOut() }} secondary/>
              </View>
            </>
  );

  return (
    <WorkspaceView style={styles.safe}>
      <StatusBar style="light" />
      <View style={styles.mobileRoot}>
        <View style={styles.mobileHeader}>
          <Pressable accessibilityRole="button" accessibilityLabel="Open Session workspace menu" accessibilityState={{ expanded: menuOpen }} onPress={() => setMenuOpen(value => !value)} style={styles.mobileMenuButton}><Text style={styles.mobileMenuIcon}>☰</Text></Pressable>
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
              <Text style={styles.muted}>Start a private workspace for your agents and sessions.</Text>
              <TextInput value={workspaceName} onChangeText={setWorkspaceName} placeholder="Workspace name" style={styles.input}/>
              <ActionButton label="Create workspace" onPress={createWorkspace} disabled={!workspaceName.trim() || busy}/>
              {workspaces.map(workspace => <Pressable key={workspace.id} onPress={() => setSelected(workspace)} style={styles.workspaceRow}><Text style={styles.rowTitle}>{workspace.name}</Text></Pressable>)}
            </View>
          ) : (
            <>
            
            {activeTab === "explore" ? (
            <ExploreScreen>
              <View style={styles.screenHeading}><Text style={styles.eyebrow}>PUBLIC DISCOVERY</Text><Text style={styles.screenTitle}>{selectedExploreAgent ? "Agent details" : "Explore agents"}</Text><Text style={styles.description}>{selectedExploreAgent ? "Review this agent's capabilities and what the Axon community says about it." : "Discover agents that their owners have deliberately published, and read the community discussion."}</Text></View>
              {selectedExploreAgent ? <View style={[styles.panel,{marginBottom:12}]}>
                <Pressable onPress={() => setSelectedExploreAgent(null)}><Text style={styles.back}>← Back to Explore</Text></Pressable>
                <Text style={styles.eyebrow}>AGENT PROFILE</Text>
                <Text style={styles.screenTitle}>{selectedExploreAgent.title}</Text>
                <Text style={styles.description}>{selectedExploreAgent.description}</Text>
                <Text style={styles.rowMeta}>{selectedExploreAgent.category} · {selectedExploreAgent.provider}</Text>
                <Text style={styles.rowMeta}>{selectedExploreAgent.like_count} likes · {selectedExploreAgent.comment_count} community comments</Text>
                <Text style={styles.muted}>Community reputation comes from member reactions and real discussion by signed-in Axon members.</Text>
              </View> : null}
              {!selectedExploreAgent ? <View style={styles.exploreSortRow}>{([{value:"recent",label:"Newest"},{value:"popular",label:"Most liked"},{value:"discussed",label:"Discussed"}] as const).map(option => <Pressable key={option.value} onPress={() => setExploreSort(option.value)} style={[styles.exploreSortChip,exploreSort===option.value && styles.agentChoiceActive]}><Text style={styles.rowTitle}>{option.label}</Text></Pressable>)}</View> : null}
              {(selectedExploreAgent ? [selectedExploreAgent] : exploreAgents).map(listing => {
                const comments = exploreComments[listing.id] ?? [];
                const roots = comments.filter(comment => !comment.parent_comment_id);
                return <View key={listing.id} style={styles.exploreCardMobile}>
                  <Pressable onPress={() => openExploreAgent(listing)} style={styles.exploreCardHeader}><View style={styles.exploreAvatar}><Text style={styles.exploreAvatarText}>{listing.title.slice(0,1).toUpperCase()}</Text></View><View style={{flex:1}}><Text style={styles.rowTitle}>{listing.title}</Text><Text style={styles.rowMeta}>{listing.category} · {listing.provider}</Text></View><Text style={styles.exploreStatus}>Published</Text></Pressable>
                  <Text style={styles.exploreDescription}>{listing.description}</Text>
                  <View style={styles.exploreMeta}><Text style={styles.rowMeta}>{listing.like_count} likes · {listing.comment_count} comments</Text><Text style={styles.rowMeta}>{new Date(listing.updated_at).toLocaleDateString()}</Text></View>
                  <View style={styles.exploreActions}><Pressable onPress={() => openExploreAgent(listing)}><Text style={styles.exploreActionText}>View details</Text></Pressable><Pressable onPress={() => toggleExploreAction(`/api/v1/explore/agents/${listing.id}/like`)}><Text style={styles.exploreActionText}>{listing.is_liked ? "♥ Liked" : "♡ Like"}</Text></Pressable><Pressable onPress={() => toggleExploreAction(`/api/v1/explore/agents/${listing.id}/follow`)}><Text style={styles.exploreActionText}>{listing.is_following ? "Following" : "Follow"}</Text></Pressable><Pressable onPress={() => startExploreConversation(listing)}><Text style={styles.exploreActionText}>Start session</Text></Pressable>{listing.is_owner ? <Pressable onPress={() => unpublishExploreAgent(listing)}><Text style={styles.exploreActionText}>Unpublish</Text></Pressable> : null}{listing.is_owner ? <Pressable onPress={() => {if(moderationOpenListing===listing.id){setModerationOpenListing(null);}else{loadModerationReports(listing.id);}}}><Text style={styles.exploreActionText}>Moderate reports</Text></Pressable> : null}<Pressable onPress={() => {setReportTarget({listingId:listing.id});setReportReason("");}}><Text style={styles.exploreActionText}>Report</Text></Pressable></View>
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
            </ExploreScreen>
          ) : mobileChatOpen && chatDetail ? (
            <>
              <View style={styles.screenHeading}><Pressable onPress={() => setMobileChatOpen(false)}><Text style={styles.back}>← Sessions</Text></Pressable><Text style={styles.screenTitle}>{chatDetail.chat.title}</Text><Text style={styles.description}>{chatDetail.messages.length} turns · Updated {new Date(chatDetail.chat.updated_at).toLocaleString()}</Text></View>
              <View style={styles.panel}>
                <View style={styles.chatAgentHeader}><View style={styles.exploreAvatar}><Text style={styles.exploreAvatarText}>A</Text></View><View style={{flex:1}}><Text style={styles.rowTitle}>{chatDetail.chat.title}</Text><Text style={styles.rowMeta}>Session</Text></View><Pressable onPress={archiveChat}><Text style={styles.back}>Archive</Text></Pressable></View>
                <View style={styles.resumeBriefMobile}><Text style={styles.fieldLabel}>APPROVED MEMORY · RESUME BRIEF</Text><Text style={styles.rowMeta}>{chatDetail.resume_brief || "No approved context yet."}</Text></View>
                {chatDetail.messages.map(message => <View key={message.id} style={[styles.messageBubbleMobile,message.role==="user" && styles.userMessageBubble]}><Text style={styles.messageSenderMobile}>{message.role === "user" ? "You" : message.role === "assistant" ? (message.answered_by || "Agent") : "System"} · {new Date(message.created_at).toLocaleString()}</Text><Text style={styles.messageTextMobile}>{message.content}</Text><ActionButton label="Index with Qoin" onPress={() => indexChatMessageWithQoin(message)} disabled={busy} secondary/></View>)}
                {chatDetail.messages.length === 0 ? <Text style={styles.muted}>No activity yet. Prompt Axon to begin.</Text> : null}
                <TextInput value={messageDraft} onChangeText={setMessageDraft} placeholder="Prompt Axon…" style={[styles.input,styles.multiline]} multiline/>
                <ActionButton label="Send prompt" onPress={sendChatMessage} disabled={!messageDraft.trim() || busy}/>
                <Text style={styles.fieldLabel}>PROJECT MEMORY</Text>
                <TextInput value={memoryDraft} onChangeText={setMemoryDraft} placeholder="Propose a decision, fact or constraint…" style={[styles.input,styles.multiline]} multiline maxLength={4000}/>
                <View style={styles.memoryTypeRow}>{(["decision","fact","constraint","open_task","note"] as const).map(kind => <Pressable key={kind} onPress={() => setMemoryType(kind)} style={[styles.memoryTypeChip,memoryType===kind && styles.agentChoiceActive]}><Text style={styles.rowTitle}>{kind.replaceAll("_"," ")}</Text></Pressable>)}</View>
                <ActionButton label="Propose memory" onPress={proposeMemory} disabled={!memoryDraft.trim() || busy} secondary/>
                {chatDetail.memory.map(entry => <View key={entry.id} style={styles.listRow}><Text style={styles.rowTitle}>{entry.type.replaceAll("_"," ")} · {entry.status}</Text><Text style={styles.rowMeta}>{entry.text}</Text>{entry.status==="proposed" ? <><ActionButton label="Approve" onPress={() => approveMemory(entry)} disabled={busy} secondary/><ActionButton label="Reject" onPress={() => changeMemory(entry,"reject")} disabled={busy} secondary/></> : null}<ActionButton label="Edit" onPress={() => {setEditingMemoryId(entry.id);setEditMemoryDraft(entry.text);}} disabled={busy} secondary/>{editingMemoryId===entry.id ? <><TextInput value={editMemoryDraft} onChangeText={setEditMemoryDraft} style={styles.input}/><ActionButton label="Save edit" onPress={() => saveMemoryEdit(entry)} disabled={busy || !editMemoryDraft.trim()}/></> : null}<ActionButton label="Delete" onPress={() => changeMemory(entry,"delete")} disabled={busy} secondary/></View>)}
              </View>
            </>
          ) : (
            <SessionPanel>
              <View style={styles.screenHeading}><Text style={styles.eyebrow}>PRIVATE WORKSPACE</Text><Text style={styles.screenTitle}>Sessions</Text><Text style={styles.description}>Sessions, agent work and project context in one place.</Text></View>
              <View style={styles.newChatRow}><TextInput value={chatTitle} onChangeText={setChatTitle} placeholder="Start a new session…" style={[styles.input,styles.newChatInput]} maxLength={160}/><ActionButton label="＋" onPress={createChat} disabled={!chatTitle.trim() || busy}/></View>
              {chats.map(chat => {
                const linkedTasks = tasks.filter(task => task.source_chat_id === chat.id);
                const progress = linkedTasks.length ? Math.round(linkedTasks.reduce((sum, task) => sum + taskProgress(task.status), 0) / linkedTasks.length) : null;
                return <SessionCard key={chat.id} session={chat} linkedTaskCount={linkedTasks.length} progress={progress} styles={styles} onOpen={() => { setActiveChat(chat.id); setMobileChatOpen(true); }} onLike={() => toggleChatLike(chat.id)} onArchive={() => archiveChatById(chat.id)} />;
              })}
              {chats.length === 0 ? <View style={styles.panel}><Text style={styles.rowTitle}>No sessions yet</Text><Text style={styles.muted}>Create your first session above to open a session.</Text></View> : null}
            </SessionPanel>
            )}
            </>
          )}
        </ScrollView>

        {menuOpen ? <View style={styles.mobileMenu} accessibilityLabel="Session workspace menu">
          <Pressable style={styles.mobileMenuItem} onPress={() => { setActiveTab("chats"); setMobileChatOpen(false); setMenuOpen(false); }}><Text style={styles.mobileMenuItemText}>Sessions</Text></Pressable>
          <Pressable style={styles.mobileMenuItem} onPress={() => { setActiveTab("explore"); setMobileChatOpen(false); setMenuOpen(false); }}><Text style={styles.mobileMenuItemText}>Explore agents</Text></Pressable>
          <Pressable style={styles.mobileMenuItem} onPress={() => { setProfileOpen(true); setMenuOpen(false); }}><Text style={styles.mobileMenuItemText}>Profile & workspace settings</Text></Pressable>
          <Pressable style={styles.mobileMenuItem} onPress={() => { setSelected(null); setActiveTab("chats"); setMenuOpen(false); }}><Text style={styles.mobileMenuItemText}>All workspaces</Text></Pressable>
          <Pressable style={styles.mobileMenuItem} onPress={() => { setMenuOpen(false); onSignOut(); }}><Text style={[styles.mobileMenuItemText, { color: "#F0A0A0" }]}>Sign out</Text></Pressable>
        </View> : null}
        {selected ? <HomeScreen activeTab={activeTab} unreadCount={chats.reduce((total, chat) => total + chat.unread_count, 0)} onTabChange={tab => { setActiveTab(tab); setMobileChatOpen(false); }} /> : null}

        {selected && !profileOpen ? (
          <>
            {quickChatOpen ? <View style={styles.quickChatComposer}>
              <Text style={styles.fieldLabel}>NEW SESSION</Text>
              <TextInput autoFocus value={chatTitle} onChangeText={setChatTitle} onSubmitEditing={createChat} returnKeyType="done" placeholder="What are we working on?" style={styles.input} maxLength={160} />
              <View style={{ flexDirection: "row", gap: 8 }}>
                <View style={{ flex: 1 }}><ActionButton label="Create session" onPress={createChat} disabled={!chatTitle.trim() || busy} /></View>
                <ActionButton label="Cancel" onPress={() => { setQuickChatOpen(false); setChatTitle(""); }} secondary />
              </View>
            </View> : null}
            <Pressable accessibilityRole="button" accessibilityLabel="Create a new session" onPress={() => { setActiveTab("chats"); setMobileChatOpen(false); setQuickChatOpen(value => !value); }} style={styles.homeFab}>
              <Text style={styles.homeFabText}>{quickChatOpen ? "×" : "+"}</Text>
            </Pressable>
          </>
        ) : null}
        <ProfileModal visible={profileOpen} onClose={() => setProfileOpen(false)}>
          {profileContent}
        </ProfileModal>


        {notificationsOpen && <NotificationsModal notifications={notifications} style={styles.notificationsOverlay} onMarkAllRead={markAllNotificationsRead} onClose={() => setNotificationsOpen(false)} onOpenNotification={openNotification} styles={styles} /> }
        {error ? <View style={styles.mobileError}><Text style={styles.errorText}>{error}</Text></View> : null}
      </View>
    </WorkspaceView>
  );
}

const styles = StyleSheet.create({
  authPage: { flexGrow: 1, justifyContent: "center", paddingHorizontal: 26, paddingVertical: 42, backgroundColor: "#0E1612" },
  authBrand: { flexDirection: "row", alignItems: "center", gap: 11, marginBottom: 42 },
  authEyebrow: { color: "#9DE4B0", fontSize: 10, fontWeight: "800", letterSpacing: 2, marginBottom: 12 },
  authTitle: { color: "#E7F6EB", fontSize: 30, lineHeight: 36, fontWeight: "700", letterSpacing: -0.8, marginBottom: 10 },
  authLabel: { color: "#B8CBBE", fontSize: 10, fontWeight: "700", letterSpacing: 1.1, marginTop: 18, marginBottom: 8 },
  authInput: { height: 52, borderRadius: 13, borderWidth: 1, borderColor: "#294536", backgroundColor: "#14231B", color: "#E7F6EB", paddingHorizontal: 15, fontSize: 14 },
  authPasswordWrap: { minHeight: 52, flexDirection: "row", alignItems: "center", borderRadius: 13, borderWidth: 1, borderColor: "#294536", backgroundColor: "#14231B" },
  authPasswordInput: { flex: 1, height: 52, color: "#E7F6EB", paddingHorizontal: 15, fontSize: 14 },
  authShowPassword: { color: "#9DE4B0", fontSize: 12, fontWeight: "700", paddingHorizontal: 15, paddingVertical: 16 },
  authForgot: { color: "#9DE4B0", fontSize: 12, fontWeight: "700", textAlign: "right", marginTop: 17 },
  authDivider: { flexDirection: "row", alignItems: "center", gap: 10, marginTop: 28, marginBottom: 15 },
  authDividerLine: { flex: 1, height: 1, backgroundColor: "#294034" },
  authDividerText: { color: "#82998A", fontSize: 9, fontWeight: "800", letterSpacing: 1 },
  authLink: { color: "#9DE4B0", fontSize: 13, fontWeight: "700", textAlign: "center", paddingVertical: 12 },
  authFootnote: { color: "#82998A", fontSize: 10, textAlign: "center", marginTop: 22 },
  authNotice: { color: "#9DE4B0", backgroundColor: "#173D29", borderRadius: 10, padding: 12, fontSize: 12, lineHeight: 18, marginTop: 12 },

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
  mobileMenuButton: { width: 34, height: 38, borderRadius: 19, alignItems: "center", justifyContent: "center", backgroundColor: "#14231B" },
  mobileMenuIcon: { color: "#9DE4B0", fontSize: 19, fontWeight: "700" },
  mobileMenu: { position: "absolute", zIndex: 42, top: 60, left: 12, width: 224, padding: 7, borderRadius: 15, backgroundColor: "#17271D", elevation: 20, shadowColor: "#000", shadowOpacity: 0.3, shadowRadius: 14, shadowOffset: { width: 0, height: 6 } },
  mobileMenuItem: { minHeight: 43, justifyContent: "center", paddingHorizontal: 13, borderRadius: 10 },
  mobileMenuItemText: { color: "#E7F6EB", fontSize: 12, fontWeight: "700" },
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
  mobileContent: { paddingHorizontal: 13, paddingTop: 10, paddingBottom: 160 },
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
  homeFab: { position: "absolute", right: 22, bottom: 88, width: 58, height: 58, borderRadius: 29, backgroundColor: "#2F8A52", alignItems: "center", justifyContent: "center", elevation: 16, shadowColor: "#000", shadowOpacity: 0.28, shadowRadius: 10, shadowOffset: { width: 0, height: 5 } },
  homeFabText: { color: "#F2FFF5", fontSize: 32, fontWeight: "400", lineHeight: 36 },
  quickChatComposer: { position: "absolute", zIndex: 30, left: 14, right: 14, bottom: 162, padding: 15, borderRadius: 18, backgroundColor: "#17271D", gap: 10, elevation: 18 },

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
  const [user, setUser] = useState<GoogleUser | null>(null);
  const [ready, setReady] = useState(false);
  const [authError, setAuthError] = useState("");
  const [working, setWorking] = useState(false);
  const googleClientIds = {
    web: process.env.EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID,
    android: process.env.EXPO_PUBLIC_GOOGLE_ANDROID_CLIENT_ID,
    ios: process.env.EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID
  };
  const activeGoogleClientId = Platform.select({
    android: googleClientIds.android,
    ios: googleClientIds.ios,
    default: googleClientIds.web
  });
  const googleAuthConfigured = Boolean(activeGoogleClientId);
  const [request, response, promptAsync] = useIdTokenAuthRequest({
    clientId: activeGoogleClientId || "not-configured.apps.googleusercontent.com",
    webClientId: googleClientIds.web,
    androidClientId: googleClientIds.android,
    iosClientId: googleClientIds.ios,
    selectAccount: true
  });

  useEffect(() => {
    let active = true;
    void AsyncStorage.getItem("axon.google.id_token").then(saved => {
      if (!active) return;
      if (saved) {
        try { googleIdToken = saved; setUser(parseGoogleUser(saved)); }
        catch { googleIdToken = null; void AsyncStorage.removeItem("axon.google.id_token"); }
      }
      setReady(true);
    }).catch(() => { if (active) setReady(true); });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (response?.type !== "success") return;
    const idToken = response.params.id_token;
    if (!idToken) { setAuthError("Google did not return a valid identity token. Please try again."); return; }
    setWorking(true);
    setAuthError("");
    try {
      const nextUser = parseGoogleUser(idToken);
      googleIdToken = idToken;
      setUser(nextUser);
      void AsyncStorage.setItem("axon.google.id_token", idToken);
    } catch (cause) {
      setAuthError(cause instanceof Error ? cause.message : "Google sign-in failed.");
    } finally { setWorking(false); }
  }, [response]);

  const signOutGoogle = () => {
    googleIdToken = null;
    setUser(null);
    setAuthError("");
    void AsyncStorage.removeItem("axon.google.id_token");
  };

  if (!ready) return <SafeAreaView style={styles.safe}><View style={styles.authPage}><ActivityIndicator/><Text style={styles.muted}>Restoring your secure session…</Text></View></SafeAreaView>;
  if (user) return <WorkspaceApp user={user} onSignOut={signOutGoogle}/>;
  if (!googleAuthConfigured) return <SafeAreaView style={styles.safe}><View style={styles.authPage}><Text style={styles.authTitle}>Google Sign-In needs setup</Text><Text style={styles.description}>Add the Google OAuth client ID for this platform to the mobile app environment, then restart Axon.</Text></View></SafeAreaView>;
  return <SafeAreaView style={styles.safe}><ScrollView contentContainerStyle={styles.authPage} keyboardShouldPersistTaps="handled"><View style={styles.authBrand}><View style={styles.logo}><Text style={styles.logoText}>A</Text></View><Text style={styles.brand}>AXON</Text></View><Text style={styles.authEyebrow}>YOUR WORK, IN ONE PLACE</Text><Text style={styles.authTitle}>Welcome to Axon</Text><Text style={styles.description}>Continue securely with your Google account. No separate Axon password required.</Text><ActionButton label={working ? "Connecting to Google…" : "Continue with Google"} onPress={()=>{setAuthError("");setWorking(true);void promptAsync().finally(()=>setWorking(false));}} disabled={working || !request}/>{authError ? <Text style={styles.errorText}>{authError}</Text> : null}<Text style={styles.authFootnote}>By continuing, you agree to the Terms &amp; Conditions and Privacy Policy.</Text></ScrollView></SafeAreaView>;
}
export default function App() { return <AuthGate />; }