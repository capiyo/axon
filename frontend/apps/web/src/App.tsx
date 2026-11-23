import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { auth } from "./firebase";

import type {
  Agent, Artifact, Health, Task, Workspace, Chat, ChatDetail, ChatMessage,
  ProjectMemoryEntry, QoinVectorSnapshot, HandoverContext, ProgressiveMemoryContext,
  QoinMemoryIndexReceipt, QoinChatMessageIndexReceipt, ExploreAgent, ExploreComment
} from "../../../packages/contracts/src";


const API_BASE_URL = (import.meta.env.VITE_AXON_API_URL ?? "").replace(/\/$/, "");

const api = async <T,>(path: string, init?: RequestInit): Promise<T> => {
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
};

function WorkspaceApp({ user }: { user: import("firebase/auth").User }) {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [selected, setSelected] = useState<Workspace | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [chats, setChats] = useState<Chat[]>([]);
  const [chatPreviews, setChatPreviews] = useState<Record<string, ChatDetail>>({});
  const [activeView, setActiveView] = useState<"chats" | "explore">("chats");
  const [profileOpen, setProfileOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [mobileConversationOpen, setMobileConversationOpen] = useState(false);
  const [exploreAgents, setExploreAgents] = useState<ExploreAgent[]>([]);
  const [exploreComments, setExploreComments] = useState<Record<string, ExploreComment[]>>({});
  const [exploreSort, setExploreSort] = useState<"recent" | "popular" | "discussed">("recent");
  const [exploreDrafts, setExploreDrafts] = useState<Record<string, string>>({});
  const [editingCommentId, setEditingCommentId] = useState<string | null>(null);
  const [editingCommentDraft, setEditingCommentDraft] = useState("");
  const [replyTarget, setReplyTarget] = useState<{ listingId: string; parentId: string } | null>(null);
  const [publishAgentId, setPublishAgentId] = useState("");
  const [publishDescription, setPublishDescription] = useState("");
  const [publishCategory, setPublishCategory] = useState("Productivity");
  const [activeChat, setActiveChat] = useState<string>("");
  const [chatDetail, setChatDetail] = useState<ChatDetail | null>(null);
  const [chatTitle, setChatTitle] = useState("");
  const [messageDraft, setMessageDraft] = useState("");
  const [memoryDraft, setMemoryDraft] = useState("");
  const [memoryType, setMemoryType] = useState<ProjectMemoryEntry["type"]>("note");
  const [editingMemoryId, setEditingMemoryId] = useState("");
  const [editMemoryDraft, setEditMemoryDraft] = useState("");
  const [name, setName] = useState("");
  const [agentName, setAgentName] = useState("");
  const [provider, setProvider] = useState("claude");
  const [artifactTitle, setArtifactTitle] = useState("");
  const [artifactContent, setArtifactContent] = useState("");
  const [taskTitle, setTaskTitle] = useState("");
  const [taskObjective, setTaskObjective] = useState("");
  const [assignee, setAssignee] = useState("");
  const [selectedArtifactIds, setSelectedArtifactIds] = useState<string[]>([]);
  const [responseDrafts, setResponseDrafts] = useState<Record<string, string>>({});
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [qoinSymbol, setQoinSymbol] = useState("BTCUSDT");
  const [qoinVector, setQoinVector] = useState<QoinVectorSnapshot | null>(null);
  const [handoverAgentId, setHandoverAgentId] = useState("");
  const [handoverTaskId, setHandoverTaskId] = useState("");
  const [handover, setHandover] = useState<HandoverContext | null>(null);
  const [progressiveContext, setProgressiveContext] = useState<ProgressiveMemoryContext | null>(null);
  const [qoinNotice, setQoinNotice] = useState("");

  async function refreshWorkspaces() {
    const [items, status] = await Promise.all([
      api<Workspace[]>("/api/v1/workspaces"),
      api<Health>("/health")
    ]);
    setWorkspaces(items);
    setHealth(status);
    setSelected(current => current ?? items[0] ?? null);
    setError("");
  }

  async function refreshWorkspace(workspace: Workspace) {
    const [agentItems, artifactItems, taskItems] = await Promise.all([
      api<Agent[]>(`/api/v1/workspaces/${workspace.id}/agents`),
      api<Artifact[]>(`/api/v1/workspaces/${workspace.id}/artifacts`),
      api<Task[]>(`/api/v1/workspaces/${workspace.id}/tasks`)
    ]);
    setAgents(agentItems);
    setArtifacts(artifactItems);
    setTasks(taskItems);
    const chatItems = await api<Chat[]>(`/api/v1/workspaces/${workspace.id}/chats`);
    setChats(chatItems);
    const previewEntries = await Promise.all(chatItems.map(async chat => {
      try { return [chat.id, await api<ChatDetail>(`/api/v1/workspaces/${workspace.id}/chats/${chat.id}`)] as const; }
      catch { return null; }
    }));
    setChatPreviews(Object.fromEntries(previewEntries.filter((entry): entry is readonly [string, ChatDetail] => entry !== null)));
    setActiveChat(current => current && chatItems.some(chat => chat.id === current) ? current : chatItems[0]?.id ?? "");
    if (activeChat && !chatItems.some(chat => chat.id === activeChat)) setChatDetail(null);
    setAssignee(current => agentItems.some(agent => agent.id === current) ? current : agentItems[0]?.id ?? "");
    setHandoverAgentId(current => agentItems.some(agent => agent.id === current) ? current : agentItems[0]?.id ?? "");
  }

  async function refreshExplore() {
    const listings = await api<ExploreAgent[]>(`/api/v1/explore/agents?sort=${exploreSort}&limit=30&offset=0`);
    setExploreAgents(listings);
    const commentEntries = await Promise.all(listings.map(async listing => {
      const comments = await api<ExploreComment[]>(`/api/v1/explore/agents/${listing.id}/comments?limit=50&offset=0`);
      return [listing.id, comments] as const;
    }));
    setExploreComments(Object.fromEntries(commentEntries));
  }

  async function publishExploreAgent(event: FormEvent<HTMLFormElement>) {
    if (!selected || !publishAgentId || !publishDescription.trim() || !publishCategory.trim()) return;
    await runForm(event, async () => {
      await api(`/api/v1/workspaces/${selected.id}/agents/${publishAgentId}/publish`, {
        method: "POST",
        body: JSON.stringify({ description: publishDescription.trim(), category: publishCategory.trim() })
      });
      setPublishDescription("");
      await refreshExplore();
    });
  }

  async function toggleExploreAction(path: string) {
    setBusy(true);
    setError("");
    try {
      await api(path, { method: "POST" });
      await refreshExplore();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Explore action failed.");
    } finally { setBusy(false); }
  }

  async function submitExploreComment(listingId: string, parentId: string | null = null) {
    const body = exploreDrafts[listingId]?.trim() ?? "";
    if (!body) return;
    setBusy(true);
    setError("");
    try {
      await api(`/api/v1/explore/agents/${listingId}/comments`, {
        method: "POST",
        body: JSON.stringify({ body, parent_comment_id: parentId })
      });
      setExploreDrafts(current => ({ ...current, [listingId]: "" }));
      setReplyTarget(null);
      await refreshExplore();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not post comment.");
    } finally { setBusy(false); }
  }

  async function toggleCommentLike(commentId: string) {
    await toggleExploreAction(`/api/v1/explore/comments/${commentId}/like`);
  }

  async function saveExploreComment(commentId: string) {
    if (!editingCommentDraft.trim()) return;
    setBusy(true);
    try {
      await api(`/api/v1/explore/comments/${commentId}`, { method: "PATCH", body: JSON.stringify({ body: editingCommentDraft.trim() }) });
      setEditingCommentId(null);
      setEditingCommentDraft("");
      await refreshExplore();
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not edit comment."); }
    finally { setBusy(false); }
  }

  async function deleteExploreComment(commentId: string) {
    setBusy(true);
    try {
      await api(`/api/v1/explore/comments/${commentId}`, { method: "DELETE" });
      await refreshExplore();
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not delete comment."); }
    finally { setBusy(false); }
  }

  async function startExploreConversation(listing: ExploreAgent) {
    if (!selected) return;
    setBusy(true);
    setError("");
    try {
      const chat = await api<Chat>(`/api/v1/workspaces/${selected.id}/chats`, {
        method: "POST",
        body: JSON.stringify({ title: listing.title })
      });
      setChats(current => [chat, ...current]);
      setActiveChat(chat.id);
      setActiveView("chats");
      setMobileConversationOpen(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not start a workspace conversation.");
    } finally { setBusy(false); }
  }

  useEffect(() => {
    void refreshWorkspaces().catch(cause => setError(cause instanceof Error ? cause.message : "Could not reach the Axon API."));
  }, []);

  useEffect(() => {
    if (activeView === "explore") {
      void refreshExplore().catch(cause => setError(cause instanceof Error ? cause.message : "Could not load Explore."));
    }
  }, [activeView, exploreSort, selected?.id]);

  useEffect(() => {
    if (selected) {
      setQoinVector(null);
      setHandover(null);
      setProgressiveContext(null);
      setQoinNotice("");
      setSelectedArtifactIds([]);
      setResponseDrafts({});
      void refreshWorkspace(selected).catch(cause => setError(cause instanceof Error ? cause.message : "Could not load workspace."));
    } else {
      setAgents([]);
      setArtifacts([]);
      setTasks([]);
    }
  }, [selected?.id]);

  useEffect(() => {
    if (!selected || !activeChat) { setChatDetail(null); return; }
    const chatId = activeChat;
    void api(`/api/v1/workspaces/${selected.id}/chats/${chatId}/read`, { method: "POST" })
      .then(() => Promise.all([
        api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${chatId}`),
        api<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`)
      ]))
      .then(([detail, items]) => { setChatDetail(detail); setChats(items); })
      .catch(cause => setError(cause instanceof Error ? cause.message : "Could not load chat."));
  }, [selected?.id, activeChat]);

  async function createChat(event: FormEvent<HTMLFormElement>) {
    if (!selected) return;
    await runForm(event, async () => {
      const chat = await api<Chat>(`/api/v1/workspaces/${selected.id}/chats`, {
        method: "POST", body: JSON.stringify({ title: chatTitle.trim() })
      });
      setChatTitle("");
      setChats(current => [chat, ...current]);
      setActiveChat(chat.id);
    });
  }

  async function archiveChat() {
    if (!selected || !activeChat) return;
    setBusy(true);
    try {
      await api(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/archive`, { method: "PATCH" });
      const remaining = await api<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`);
      setChats(remaining);
      setChatDetail(null);
      setActiveChat(remaining[0]?.id ?? "");
      setMobileConversationOpen(false);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not archive chat."); }
    finally { setBusy(false); }
  }

  async function toggleChatLike(chatId: string) {
    if (!selected) return;
    try {
      await api(`/api/v1/workspaces/${selected.id}/chats/${chatId}/like`, { method: "POST" });
      setChats(await api<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not update chat reaction.");
    }
  }

  async function archiveChatById(chatId: string) {
    if (!selected) return;
    setBusy(true);
    try {
      await api(`/api/v1/workspaces/${selected.id}/chats/${chatId}/archive`, { method: "PATCH" });
      await refreshWorkspace(selected);
      if (activeChat === chatId) setMobileConversationOpen(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not archive chat.");
    } finally { setBusy(false); }
  }

  async function sendChatMessage(event: FormEvent<HTMLFormElement>) {
    if (!selected || !activeChat || !messageDraft.trim()) return;
    await runForm(event, async () => {
      await api<ChatMessage>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/messages`, {
        method: "POST", body: JSON.stringify({ role: "user", content: messageDraft.trim() })
      });
      setMessageDraft("");
      const [detail, chatItems] = await Promise.all([
        api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`),
        api<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`)
      ]);
      setChatDetail(detail);
      setChats(chatItems);
    });
  }

  async function proposeMemory(event: FormEvent<HTMLFormElement>) {
    if (!selected || !activeChat || !memoryDraft.trim()) return;
    await runForm(event, async () => {
      await api<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory`, {
        method: "POST", body: JSON.stringify({ type: memoryType, text: memoryDraft.trim() })
      });
      setMemoryDraft("");
      setChatDetail(await api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    });
  }

  async function saveMemoryEdit(entry: ProjectMemoryEntry) {
    if (!selected || !activeChat || !editMemoryDraft.trim()) return;
    setBusy(true);
    try {
      await api<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory/${entry.id}/edit`, {
        method: "POST", body: JSON.stringify({ text: editMemoryDraft.trim(), reason: entry.reason })
      });
      setEditingMemoryId("");
      setEditMemoryDraft("");
      setChatDetail(await api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Memory edit failed."); }
    finally { setBusy(false); }
  }

  async function reviewMemory(entry: ProjectMemoryEntry, action: "approve" | "reject" | "delete") {
    if (!selected || !activeChat) return;
    setBusy(true);
    setError("");
    try {
      await api<ProjectMemoryEntry>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/memory/${entry.id}/${action}`, { method: "POST" });
      setChatDetail(await api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Memory update failed.");
    } finally { setBusy(false); }
  }


  async function loadQoinVector() {
    if (!selected || !qoinSymbol.trim()) return;
    setBusy(true);
    setError("");
    setQoinNotice("");
    try {
      const snapshot = await api<QoinVectorSnapshot>(
        `/api/v1/workspaces/${selected.id}/memory/qoin-vector?symbol=${encodeURIComponent(qoinSymbol.trim())}`
      );
      setQoinVector(snapshot);
      setQoinNotice(`Loaded ${snapshot.symbol} · ${snapshot.vector.model_version}`);
      setHandover(null);
      setProgressiveContext(null);
    } catch (cause) {
      setQoinVector(null);
      setError(cause instanceof Error ? cause.message : "Could not load the Qoin market vector.");
    } finally { setBusy(false); }
  }

  async function indexChatMessageWithQoin(message: ChatMessage) {
    if (!selected || !activeChat || !qoinSymbol.trim()) return;
    setBusy(true);
    setError("");
    try {
      const receipt = await api<QoinChatMessageIndexReceipt>(
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
      setQoinNotice(`Indexed chat message ${receipt.message_id.slice(0, 8)} using ${receipt.symbol} at ${new Date(receipt.qoin_event_ts_ms).toLocaleString()} · available after the next hourly epoch.`);
      setError("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not index this chat message with Qoin.");
    } finally { setBusy(false); }
  }

  async function indexMemoryWithQoin(entry: ProjectMemoryEntry) {
    if (!selected || !activeChat || !qoinSymbol.trim()) return;
    setBusy(true);
    setError("");
    try {
      const receipt = await api<QoinMemoryIndexReceipt>(
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
      setQoinNotice(`Indexed ${receipt.memory_id.slice(0, 8)} using ${receipt.symbol} at ${new Date(receipt.qoin_event_ts_ms).toLocaleString()} · available after the next hourly epoch.`);
      setError("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not index this memory with Qoin.");
    } finally { setBusy(false); }
  }

  async function prepareAgentHandover() {
    if (!selected || !handoverAgentId || !qoinVector) return;
    setBusy(true);
    setError("");
    try {
      const result = await api<HandoverContext>(
        `/api/v1/workspaces/${selected.id}/agents/${handoverAgentId}/memory-context`,
        {
          method: "POST",
          body: JSON.stringify({
            workspace_id: selected.id,
            agent_id: handoverAgentId,
            task_id: handoverTaskId || null,
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
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not prepare agent memory context.");
    } finally { setBusy(false); }
  }

  async function retrieveMoreContext() {
    if (!selected || !handoverAgentId || !handover || !qoinVector) return;
    setBusy(true);
    setError("");
    try {
      const result = await api<ProgressiveMemoryContext>(
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
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not retrieve additional context.");
    } finally { setBusy(false); }
  }

  async function runForm(event: FormEvent<HTMLFormElement>, action: () => Promise<void>) {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The request failed.");
    } finally {
      setBusy(false);
    }
  }

  async function createWorkspace(event: FormEvent<HTMLFormElement>) {
    await runForm(event, async () => {
      const created = await api<Workspace>("/api/v1/workspaces", {
        method: "POST",
        body: JSON.stringify({ name: name.trim() })
      });
      setName("");
      await refreshWorkspaces();
      setSelected(created);
    });
  }

  async function createAgent(event: FormEvent<HTMLFormElement>) {
    if (!selected) return;
    await runForm(event, async () => {
      await api<Agent>(`/api/v1/workspaces/${selected.id}/agents`, {
        method: "POST",
        body: JSON.stringify({ display_name: agentName.trim(), provider: provider.trim() })
      });
      setAgentName("");
      await refreshWorkspace(selected);
    });
  }

  async function createArtifact(event: FormEvent<HTMLFormElement>) {
    if (!selected) return;
    await runForm(event, async () => {
      await api<Artifact>(`/api/v1/workspaces/${selected.id}/artifacts`, {
        method: "POST",
        body: JSON.stringify({ title: artifactTitle.trim(), content: artifactContent.trim(), source_agent_id: null })
      });
      setArtifactTitle("");
      setArtifactContent("");
      await refreshWorkspace(selected);
    });
  }

  async function createTask(event: FormEvent<HTMLFormElement>) {
    if (!selected || !assignee) return;
    await runForm(event, async () => {
      await api<Task>(`/api/v1/workspaces/${selected.id}/tasks`, {
        method: "POST",
        body: JSON.stringify({
          title: taskTitle.trim(),
          objective: taskObjective.trim(),
          assignee_agent_id: assignee,
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
    setBusy(true);
    setError("");
    try {
      await api<Task>(`/api/v1/tasks/${task.id}/transitions`, {
        method: "POST",
        body: JSON.stringify({
          expected_version: task.version,
          status,
          note: `Moved to ${status.replaceAll("_", " ")} from the workspace interface.`
        })
      });
      await refreshWorkspace(selected);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Task update failed.");
    } finally {
      setBusy(false);
    }
  }

  async function submitTaskResponse(task: Task) {
    if (!selected) return;
    const content = responseDrafts[task.id]?.trim() ?? "";
    if (!content || busy) return;
    setBusy(true);
    setError("");
    try {
      await api(`/api/v1/tasks/${task.id}/submit`, {
        method: "POST",
        body: JSON.stringify({
          expected_version: task.version,
          title: `${task.title} — response`,
          content
        })
      });
      setResponseDrafts(current => ({ ...current, [task.id]: "" }));
      await refreshWorkspace(selected);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Response submission failed.");
    } finally {
      setBusy(false);
    }
  }

  const nextStatus: Record<string, string> = {
    offered: "acknowledged",
    acknowledged: "in_progress",
    in_progress: "submitted",
    submitted: "review_required",
    review_required: "accepted",
    changes_requested: "in_progress",
    accepted: "completed"
  };

  return (
    <main className="shell">
      <aside className="rail profile-sidebar">
        <div className="profile-identity">
          <div className="profile-avatar">{user.photoURL ? <img src={user.photoURL} alt="" /> : (user.displayName || user.email || "A").slice(0, 1).toUpperCase()}</div>
          <div className="profile-identity-copy"><strong>{user.displayName || user.email || "Axon user"}</strong><small>{selected?.name ?? "Personal workspace"}</small></div>
          <span className="profile-presence" title="Signed in" />
        </div>
        <div className="profile-section"><span className="profile-section-label">WORKSPACE OVERVIEW</span><div className="profile-stat-grid">
          <div><strong>{agents.length}</strong><small>Agents</small></div><div><strong>{chats.length}</strong><small>Chats</small></div>
          <div><strong>{tasks.filter(task => task.status === "in_progress").length}</strong><small>Running</small></div><div><strong>{tasks.filter(task => task.status === "completed").length}</strong><small>Completed</small></div>
          <div><strong>{tasks.filter(task => task.status === "offered" || task.status === "acknowledged").length}</strong><small>Queued</small></div><div><strong>{artifacts.length}</strong><small>Artifacts</small></div>
        </div></div>
        <div className="profile-section"><span className="profile-section-label">MEMORY & CONTEXT</span><div className="profile-metric"><span>Approved memory entries</span><strong>{Object.values(chatPreviews).reduce((total, detail) => total + detail.memory.filter(entry => entry.status === "approved").length, 0)}</strong></div><div className="profile-metric"><span>Resume briefs available</span><strong>{Object.values(chatPreviews).filter(detail => Boolean(detail.resume_brief)).length}</strong></div><div className="profile-metric"><span>Messages in loaded chats</span><strong>{Object.values(chatPreviews).reduce((total, detail) => total + detail.messages.length, 0)}</strong></div></div>
        <div className="profile-section"><span className="profile-section-label">EXECUTION & HEALTH</span><div className="profile-metric"><span>Awaiting review</span><strong>{tasks.filter(task => task.status === "review_required" || task.status === "submitted").length}</strong></div><div className="profile-metric"><span>Rejected / cancelled</span><strong>{tasks.filter(task => task.status === "rejected" || task.status === "cancelled").length}</strong></div><div className="profile-metric"><span>API status</span><strong>{health?.status === "ok" ? "Connected" : "Unavailable"}</strong></div><div className="profile-metric"><span>Storage</span><strong>{health?.storage === "persistent" ? "Persistent" : health?.storage ?? "Unknown"}</strong></div></div>
        <div className="profile-section"><span className="profile-section-label">WORKSPACE RESOURCES</span><div className="profile-metric"><span>Captured artifacts</span><strong>{artifacts.length}</strong></div><div className="profile-metric"><span>Total tasks</span><strong>{tasks.length}</strong></div><div className="profile-metric"><span>Conversation archive</span><strong>{chats.length} active</strong></div></div>
        <div className="profile-section profile-activity"><span className="profile-section-label">RECENT WORK</span>{tasks.slice().sort((a,b)=>b.updated_at.localeCompare(a.updated_at)).slice(0,4).map(task => <div className="profile-activity-row" key={task.id}><span className={`activity-dot activity-${task.status}`} /><div><strong>{task.title}</strong><small>{task.status.replaceAll("_"," ")} · {new Date(task.updated_at).toLocaleDateString()}</small></div></div>)}{tasks.length === 0 && <p className="profile-empty">Task activity will appear here as work is assigned.</p>}</div>
        <div className="profile-bottom"><span>AXON WORKSPACE</span><small>Private · Firebase authenticated</small></div>
      </aside>
      <section className="content">
        <header className="topbar">
          <div className="topbar-brand"><span className="brand-mark">A</span><div><p className="eyebrow">AXON WORKSPACE</p><h1>{selected?.name ?? "Your workspace"}</h1></div></div>
          <div className="topbar-actions"><div className="status"><span className={health?.status === "ok" ? "pulse online" : "pulse"} />{health?.status === "ok" ? "Connected" : "API unavailable"}</div><div className="app-menu-anchor"><button type="button" className="topbar-action" onClick={() => setMenuOpen(value => !value)} aria-expanded={menuOpen}>Menu</button>{menuOpen && <div className="app-menu"><button type="button" onClick={() => {setActiveView("explore");setMenuOpen(false);}}>Explore agents</button><button type="button" onClick={() => {setProfileOpen(true);setMenuOpen(false);}}>Profile & workspace</button><button type="button" onClick={() => {setActiveView("chats");setMenuOpen(false);}}>Chats</button></div>}</div><button type="button" className="topbar-profile" onClick={() => setProfileOpen(true)} aria-label="Open profile">{user.photoURL ? <img src={user.photoURL} alt="" /> : (user.displayName || user.email || "A").slice(0,1).toUpperCase()}</button></div>
        </header>

        {!selected ? (
          <>
            <section className="intro">
              <div>
                <span className="kicker">YOUR SHARED WORKSPACE</span>
                <h2>Give every agent the context it needs.</h2>
                <p>Capture work once. Hand it off with its evidence, decisions and history intact.</p>
              </div>
              <div className="orbit" aria-hidden="true">
                <span className="orbit-core">A</span>
                <span className="orbit-node node-a">A</span>
                <span className="orbit-node node-b">B</span>
                <span className="orbit-node node-c">↗</span>
              </div>
            </section>
            <section className="section-heading">
              <div><span className="kicker">START HERE</span><h2>Projects and workspaces</h2></div>
              <span className="count">{workspaces.length.toString().padStart(2, "0")} WORKSPACES</span>
            </section>
            <form className="create-card" onSubmit={createWorkspace}>
              <div className="create-icon">＋</div>
              <div className="create-copy">
                <label htmlFor="workspace-name">Create a workspace</label>
                <span>Start with a project, a problem, or a team of agents.</span>
              </div>
              <input id="workspace-name" value={name} onChange={event => setName(event.target.value)} placeholder="e.g. Funspot engineering" maxLength={120} required />
              <button disabled={!name.trim() || busy}>{busy ? "Creating…" : "Create workspace"}</button>
            </form>
            <div className="workspace-grid">
              {workspaces.map(workspace => (
                <button className="workspace-card workspace-button" key={workspace.id} onClick={() => setSelected(workspace)}>
                  <div className="card-top"><span className="project-glyph">{workspace.name.slice(0, 1).toUpperCase()}</span><span className="arrow">↗</span></div>
                  <h3>{workspace.name}</h3>
                  <p>Created {new Date(workspace.created_at).toLocaleDateString()}</p>
                  <div className="card-foot"><span>OPEN WORKSPACE</span><span className="quiet-dot" /></div>
                </button>
              ))}
              {workspaces.length === 0 && !error && (
                <div className="empty-state">
                  <div className="empty-symbol">◎</div>
                  <h3>Your first workspace starts here.</h3>
                  <p>Create a space for Funspot, Qoin, or any problem where multiple agents need to work together.</p>
                </div>
              )}
            </div>
          </>
        ) : (
          <>
            <div className="workspace-header">
              <button className="back-link" onClick={() => setSelected(null)}>← All workspaces</button>
              <span className="count">PROJECT WORKSPACE</span>
              <h2>{selected.name}</h2>
              <p>Register agents, capture their work, and create explicit handoffs.</p>
            </div>
            {activeView === "chats" ? <section className="panel chat-panel">
              <div className="panel-heading"><div><span className="kicker">04 · PROJECT CONVERSATIONS</span><h3>Chats & memory</h3></div><span className="count">{chats.length} CHATS</span></div>
              <div className={mobileConversationOpen ? "chat-layout mobile-conversation-open" : "chat-layout"}>
                <aside className="chat-sidebar">
                  <form className="stack-form" onSubmit={createChat}>
                    <label htmlFor="chat-title">New chat</label>
                    <input id="chat-title" value={chatTitle} onChange={event => setChatTitle(event.target.value)} placeholder="What are we working on?" maxLength={160} required />
                    <button disabled={!chatTitle.trim() || busy}>Create chat</button>
                  </form>
                  <div className="chat-list">
                    {chats.map(chat => { const preview = chatPreviews[chat.id]; const lastMessage = preview?.messages[preview.messages.length - 1]; return <button type="button" className={activeChat === chat.id ? "chat-list-item active" : "chat-list-item"} key={chat.id} onClick={() => { setActiveChat(chat.id); setMobileConversationOpen(true); }}>
                      <span className="chat-card-avatar">{(lastMessage?.answered_by || chat.title || "A").slice(0,1).toUpperCase()}</span><span className="chat-card-content"><span className="chat-card-heading"><strong>{chat.title}</strong><small>{new Date(chat.updated_at).toLocaleString()}</small></span><small className="chat-card-agent">{lastMessage?.answered_by || "Workspace conversation"}</small><span className="chat-card-preview">{lastMessage?.content || "No messages yet. Open this conversation to begin."}</span><span className="chat-card-footer"><small>{preview ? `${preview.messages.length} messages` : "Loading conversation"} · {lastMessage?.role === "assistant" ? "Agent replied" : lastMessage?.role === "user" ? "Your message" : "New conversation"}</small><span className="chat-card-actions">{chat.unread_count > 0 && <span className="chat-unread-count">{chat.unread_count}</span>}<span role="button" tabIndex={0} className="chat-card-like" onClick={event => {event.stopPropagation();void toggleChatLike(chat.id);}} onKeyDown={event => {if(event.key==="Enter" || event.key===" "){event.preventDefault();event.stopPropagation();void toggleChatLike(chat.id);}}}>{chat.is_liked ? "♥" : "♡"}</span><span role="button" tabIndex={0} className="chat-card-archive" onClick={event => {event.stopPropagation();void archiveChatById(chat.id);}} onKeyDown={event => {if(event.key==="Enter" || event.key===" "){event.preventDefault();event.stopPropagation();void archiveChatById(chat.id);}}}>Archive</span></span></span></span>
                    </button>; })}
                    {chats.length === 0 && <div className="chat-list-empty"><span className="chat-empty-mark">＋</span><strong>No conversations yet</strong><p>Create a titled conversation to start capturing project discussions and decisions.</p></div>}
                  </div>
                </aside>
                <div className="chat-main">
                  {!chatDetail ? <div className="chat-welcome"><span className="chat-welcome-mark">✳</span><span className="kicker">WORKSPACE MESSAGING</span><h3>Give this workspace a voice.</h3><p>Choose a conversation or create one to exchange updates, record decisions and build shared context for your agents.</p><div className="chat-welcome-note"><strong>{selected.name}</strong><span>Shared project workspace</span></div></div> : <>
                    <div className="chat-mobile-back"><button type="button" onClick={() => setMobileConversationOpen(false)}>← Chats</button></div><div className="chat-conversation-heading"><div><span className="kicker">CONVERSATION · {selected.name}</span><h3>{chatDetail.chat.title}</h3><p>{chatDetail.messages.length} {chatDetail.messages.length === 1 ? "message" : "messages"} · Updated {new Date(chatDetail.chat.updated_at).toLocaleString()}</p></div><span className="chat-live-mark">●</span></div>
                    <div className="resume-brief"><div className="panel-heading"><span className="kicker">RESUME BRIEF · APPROVED MEMORY ONLY</span><button type="button" className="secondary" disabled={busy} onClick={() => void archiveChat()}>Archive chat</button></div><p>{chatDetail.resume_brief || "No context yet. Add messages and approve useful memory to build a concise handoff."}</p></div>
                    <div className="chat-messages">{chatDetail.messages.map(message => <article className={`chat-message message-${message.role}`} key={message.id}><div className="chat-message-heading"><strong>{message.role === "user" ? "You" : message.role === "assistant" ? "Axon assistant" : "System"}</strong><small>{message.answered_by ? ` · ${message.answered_by} · ` : " · "}{new Date(message.created_at).toLocaleString()}</small></div><p>{message.content}</p><button type="button" className="secondary qoin-message-index" disabled={busy} onClick={() => void indexChatMessageWithQoin(message)}>Index with Qoin</button></article>)}{chatDetail.messages.length === 0 && <div className="chat-first-message"><span>✳</span><h4>{chatDetail.chat.title}</h4><p>This conversation is ready. Send the first message to give your agents a shared starting point.</p></div>}</div>
                    <form className="chat-compose" onSubmit={sendChatMessage}><textarea value={messageDraft} onChange={event => setMessageDraft(event.target.value)} rows={3} maxLength={256000} placeholder="Write a message to this project chat…" required /><button disabled={!messageDraft.trim() || busy}>Send message</button></form>
                    <div className="memory-section">
                      <div className="panel-heading"><div><span className="kicker">PROJECT MEMORY</span><h3>Review proposed knowledge</h3></div></div>
                      <form className="memory-compose" onSubmit={proposeMemory}><select value={memoryType} onChange={event => setMemoryType(event.target.value as ProjectMemoryEntry["type"])}><option value="decision">Decision</option><option value="fact">Fact</option><option value="constraint">Constraint</option><option value="open_task">Open task</option><option value="note">Note</option></select><input value={memoryDraft} onChange={event => setMemoryDraft(event.target.value)} placeholder="Capture a decision, fact, constraint or open task…" maxLength={4000} required /><button disabled={!memoryDraft.trim() || busy}>Propose memory</button></form>
                      {chatDetail.memory.map(entry => <article className="memory-item" key={entry.id}><div><strong>{entry.type.replaceAll("_", " ")}</strong><small> · {entry.status}</small></div>{editingMemoryId === entry.id ? <div className="memory-compose"><input value={editMemoryDraft} onChange={event => setEditMemoryDraft(event.target.value)} maxLength={4000} /><button type="button" disabled={busy || !editMemoryDraft.trim()} onClick={() => void saveMemoryEdit(entry)}>Save</button><button type="button" className="secondary" onClick={() => setEditingMemoryId("")}>Cancel</button></div> : <p>{entry.text}</p>}<div className="memory-actions">{entry.status === "proposed" && <><button type="button" disabled={busy} onClick={() => void reviewMemory(entry, "approve")}>Approve</button><button type="button" className="secondary" disabled={busy} onClick={() => void reviewMemory(entry, "reject")}>Reject</button></>}<button type="button" className="secondary" disabled={busy} onClick={() => { setEditingMemoryId(entry.id); setEditMemoryDraft(entry.text); }}>Edit</button><button type="button" className="secondary" disabled={busy || entry.status !== "approved"} onClick={() => void indexMemoryWithQoin(entry)}>Index with Qoin</button><button type="button" className="secondary" disabled={busy} onClick={() => void reviewMemory(entry, "delete")}>Delete</button></div></article>)}
                      {chatDetail.memory.length === 0 && <p className="muted">No project memory proposed yet. Only approved entries shape future resume briefs.</p>}
                    </div>
                  </>}
                </div>
              </div>
            </section> : <section className="panel explore-panel">
              <div className="panel-heading"><div><span className="kicker">PUBLIC AGENT DISCOVERY</span><h3>Explore agents</h3></div><label className="explore-sort">Sort <select value={exploreSort} onChange={event => setExploreSort(event.target.value as "recent" | "popular" | "discussed")}><option value="recent">Newest</option><option value="popular">Most liked</option><option value="discussed">Most discussed</option></select></label></div>
              <form className="explore-publish-form" onSubmit={publishExploreAgent}>
                <strong>Publish one of your agents</strong>
                <p>Publishing is explicit. Only the selected agent's public description and provider label are exposed; workspace chats, artifacts and memory remain private.</p>
                <div className="explore-publish-fields"><select value={publishAgentId} onChange={event => setPublishAgentId(event.target.value)} required><option value="">Choose your agent</option>{agents.map(agent => <option key={agent.id} value={agent.id}>{agent.display_name}</option>)}</select><input value={publishCategory} onChange={event => setPublishCategory(event.target.value)} placeholder="Category" maxLength={80} required /></div>
                <textarea value={publishDescription} onChange={event => setPublishDescription(event.target.value)} placeholder="Describe what this agent does and who it helps…" maxLength={4000} rows={3} required />
                <button disabled={busy || !publishAgentId || !publishDescription.trim()}>Publish agent</button>
              </form>
              <div className="explore-card-list">
                {exploreAgents.map(listing => {
                  const comments = exploreComments[listing.id] ?? [];
                  const roots = comments.filter(comment => !comment.parent_comment_id);
                  return <article className="explore-agent-card" key={listing.id}>
                    <div className="explore-agent-top"><span className="chat-card-avatar">{listing.title.slice(0,1).toUpperCase()}</span><div><strong>{listing.title}</strong><small>{listing.category} · {listing.provider}</small></div><span className="agent-availability">Published</span></div>
                    <p>{listing.description}</p>
                    <div className="explore-agent-meta"><span>{listing.like_count} likes · {listing.comment_count} comments</span><span>Updated {new Date(listing.updated_at).toLocaleDateString()}</span></div>
                    <div className="explore-social-actions"><button type="button" onClick={() => void toggleExploreAction(`/api/v1/explore/agents/${listing.id}/like`)} disabled={busy}>{listing.is_liked ? "♥ Liked" : "♡ Like"}</button><button type="button" onClick={() => void toggleExploreAction(`/api/v1/explore/agents/${listing.id}/follow`)} disabled={busy}>{listing.is_following ? "Following" : "Follow"}</button><button type="button" onClick={() => void startExploreConversation(listing)} disabled={busy}>Start workspace chat</button><span>Public listing</span></div>
                    <div className="explore-discussion"><strong>Community discussion</strong>
                      {roots.map(comment => <div className="explore-comment" key={comment.id}>
                        <span className="explore-comment-avatar">A</span><div className="explore-comment-body"><div className="explore-comment-heading"><strong>{comment.author_label}</strong><small>{new Date(comment.created_at).toLocaleString()}</small></div>{editingCommentId === comment.id ? <form className="explore-edit-form" onSubmit={event => {event.preventDefault();void saveExploreComment(comment.id);}}><textarea value={editingCommentDraft} onChange={event => setEditingCommentDraft(event.target.value)} rows={3}/><button disabled={busy || !editingCommentDraft.trim()}>Save</button><button type="button" onClick={() => setEditingCommentId(null)}>Cancel</button></form> : <p>{comment.body}</p>}<div className="explore-comment-actions"><button type="button" onClick={() => void toggleCommentLike(comment.id)} disabled={busy}>{comment.is_liked ? "♥" : "♡"} {comment.like_count}</button><button type="button" onClick={() => setReplyTarget(replyTarget?.parentId === comment.id ? null : { listingId: listing.id, parentId: comment.id })}>Reply · {comment.reply_count}</button>{comment.is_mine && <><button type="button" onClick={() => {setEditingCommentId(comment.id);setEditingCommentDraft(comment.body);}}>Edit</button><button type="button" onClick={() => void deleteExploreComment(comment.id)} disabled={busy}>Delete</button></>}</div>
                          {comments.filter(reply => reply.parent_comment_id === comment.id).map(reply => <div className="explore-reply" key={reply.id}><div className="explore-comment-heading"><strong>{reply.author_label}</strong><small>{new Date(reply.created_at).toLocaleString()}</small></div>{editingCommentId === reply.id ? <form className="explore-edit-form" onSubmit={event => {event.preventDefault();void saveExploreComment(reply.id);}}><textarea value={editingCommentDraft} onChange={event => setEditingCommentDraft(event.target.value)} rows={2}/><button disabled={busy || !editingCommentDraft.trim()}>Save</button><button type="button" onClick={() => setEditingCommentId(null)}>Cancel</button></form> : <p>{reply.body}</p>}<button type="button" onClick={() => void toggleCommentLike(reply.id)} disabled={busy}>{reply.is_liked ? "♥" : "♡"} {reply.like_count}</button>{reply.is_mine && <span className="explore-own-actions"><button type="button" onClick={() => {setEditingCommentId(reply.id);setEditingCommentDraft(reply.body);}}>Edit</button><button type="button" onClick={() => void deleteExploreComment(reply.id)} disabled={busy}>Delete</button></span>}</div>)}
                          {replyTarget?.listingId === listing.id && replyTarget.parentId === comment.id && <form className="explore-reply-form" onSubmit={event => { event.preventDefault(); void submitExploreComment(listing.id, comment.id); }}><input value={exploreDrafts[listing.id] ?? ""} onChange={event => setExploreDrafts(current => ({...current,[listing.id]:event.target.value}))} placeholder="Write a reply…" maxLength={5000}/><button disabled={busy || !(exploreDrafts[listing.id] ?? "").trim()}>Reply</button></form>}
                        </div></div>)}
                      {roots.length === 0 && <p className="explore-no-comments">No community comments yet. Start the discussion.</p>}
                      <form className="explore-comment-form" onSubmit={event => { event.preventDefault(); void submitExploreComment(listing.id); }}><input value={replyTarget?.listingId === listing.id ? "" : exploreDrafts[listing.id] ?? ""} onChange={event => setExploreDrafts(current => ({...current,[listing.id]:event.target.value}))} placeholder="Write a comment…" maxLength={5000}/><button disabled={busy || !(exploreDrafts[listing.id] ?? "").trim()}>Post</button></form>
                    </div>
                  </article>;
                })}
                {exploreAgents.length === 0 && <div className="chat-list-empty"><strong>No public agents yet</strong><p>Publish an agent from this workspace to make it discoverable to the Axon community.</p></div>}
              </div>
            </section>}

            <section className="panel qoin-memory-panel">
              <div className="panel-heading">
                <div><span className="kicker">05 · QOIN × AXON</span><h3>Market-aware historical memory</h3></div>
                <span className="count">QKV + QOIN VECTOR</span>
              </div>
              <p className="muted">Qoin supplies its six-dimensional attended market-state vector. Axon uses it as the query/key context for historical decisions; it is a market-state vector, not a text embedding.</p>
              <div className="qoin-bridge-controls">
                <label>Market symbol<input value={qoinSymbol} onChange={event => { setQoinSymbol(event.target.value.toUpperCase()); setQoinVector(null); setHandover(null); setProgressiveContext(null); }} maxLength={32} placeholder="BTCUSDT" /></label>
                <button type="button" disabled={busy || !qoinSymbol.trim()} onClick={() => void loadQoinVector()}>{busy ? "Loading…" : "Load latest Qoin vector"}</button>
              </div>
              {qoinNotice && <p className="qoin-notice" role="status">{qoinNotice}</p>}
              {qoinVector && <div className="qoin-snapshot">
                <div className="qoin-snapshot-head"><strong>{qoinVector.symbol}</strong><span>Model {qoinVector.vector.model_version}</span><span>Signal {qoinVector.action.toUpperCase()}</span></div>
                <div className="qoin-features">{Object.entries(qoinVector.features).map(([name, value]) => <span key={name}>{name.replaceAll("_", " ")} <strong>{value.toFixed(3)}</strong></span>)}</div>
                <p>{qoinVector.reason}</p>
              </div>}
              <div className="handover-controls">
                <label>Receiving agent<select value={handoverAgentId} onChange={event => { setHandoverAgentId(event.target.value); setHandover(null); setProgressiveContext(null); }}><option value="">Choose an agent</option>{agents.map(agent => <option key={agent.id} value={agent.id}>{agent.display_name} · {agent.provider}</option>)}</select></label>
                <label>Optional task scope<select value={handoverTaskId} onChange={event => setHandoverTaskId(event.target.value)}><option value="">Workspace-wide context</option>{tasks.filter(task => task.assignee_agent_id === handoverAgentId).map(task => <option key={task.id} value={task.id}>{task.title}</option>)}</select></label>
                <button type="button" disabled={busy || !qoinVector || !handoverAgentId} onClick={() => void prepareAgentHandover()}>Prepare agent's first 20% context</button>
              </div>
              {handover && <div className="handover-result">
                <div className="panel-heading"><div><span className="kicker">PINNED MEMORY EPOCH {handover.epoch.epoch_number}</span><h4>Initial context loaded</h4></div><span className="count">{handover.initial.length} MEMORIES</span></div>
                <div className="qoin-coverage"><strong>{(handover.achieved_coverage * 100).toFixed(1)}%</strong><span>measured contribution coverage · {handover.deferred_count} deferred</span>{handover.budget_exhausted && <small>20% context budget reached before the 80% target.</small>}</div>
                {handover.initial.map(item => <details className="handover-memory" key={item.memory_id}><summary><strong>{item.source_kind}</strong><span>Attention {(item.attention_score * 100).toFixed(1)}%</span></summary><code>{item.payload_json}</code></details>)}
                <button type="button" className="secondary" disabled={busy || handover.deferred_count === 0} onClick={() => void retrieveMoreContext()}>Retrieve next 10 historical memories</button>
                {progressiveContext && <div className="progressive-result"><strong>Progressive retrieval · {progressiveContext.memories.length} added · {progressiveContext.remaining_candidates} remaining</strong>{progressiveContext.memories.map(item => <details className="handover-memory" key={item.memory_id}><summary>{item.source_kind} · {(item.attention_score * 100).toFixed(1)}% attention</summary><code>{item.payload_json}</code></details>)}</div>}
              </div>}
            </section>

            <div className="workspace-columns">
              <section className="panel">
                <div className="panel-heading"><div><span className="kicker">01 · PARTICIPANTS</span><h3>Agents</h3></div><span className="count">{agents.length}</span></div>
                <form className="stack-form" onSubmit={createAgent}>
                  <label htmlFor="agent-name">Agent name</label>
                  <input id="agent-name" value={agentName} onChange={event => setAgentName(event.target.value)} placeholder="e.g. Architecture reviewer" maxLength={100} required />
                  <label htmlFor="provider">Provider label</label>
                  <select id="provider" value={provider} onChange={event => setProvider(event.target.value)}>
                    <option value="claude">Claude</option><option value="chatgpt">ChatGPT</option><option value="gemini">Gemini</option><option value="local">Local model</option><option value="other">Other / manual</option>
                  </select>
                  <button disabled={!agentName.trim() || busy}>{busy ? "Saving…" : "Register agent"}</button>
                </form>
                <div className="item-list">
                  {agents.map(agent => <div className="agent-item" key={agent.id}><span className="agent-avatar">{agent.display_name.slice(0,1).toUpperCase()}</span><div><strong>{agent.display_name}</strong><small>{agent.provider} · manual connection</small></div></div>)}
                  {agents.length === 0 && <p className="muted">No agents yet. Register the first participant above.</p>}
                </div>
              </section>

              <section className="panel">
                <div className="panel-heading"><div><span className="kicker">02 · SHARED KNOWLEDGE</span><h3>Artifacts</h3></div><span className="count">{artifacts.length}</span></div>
                <form className="stack-form" onSubmit={createArtifact}>
                  <label htmlFor="artifact-title">Artifact title</label>
                  <input id="artifact-title" value={artifactTitle} onChange={event => setArtifactTitle(event.target.value)} placeholder="e.g. Bug investigation notes" maxLength={200} required />
                  <label htmlFor="artifact-content">Captured content</label>
                  <textarea id="artifact-content" value={artifactContent} onChange={event => setArtifactContent(event.target.value)} placeholder="Paste the relevant response, findings, or project context…" maxLength={512000} rows={4} required />
                  <button disabled={!artifactTitle.trim() || !artifactContent.trim() || busy}>{busy ? "Saving…" : "Capture artifact"}</button>
                </form>
                <div className="item-list">
                  {artifacts.map(artifact => <details className="artifact-item" key={artifact.id}><summary><strong>{artifact.title}</strong><small>{new Date(artifact.captured_at).toLocaleString()}</small></summary><p>{artifact.content}</p></details>)}
                  {artifacts.length === 0 && <p className="muted">Captured outputs and evidence will appear here.</p>}
                </div>
              </section>
            </div>

            <section className="panel task-panel">
              <div className="panel-heading"><div><span className="kicker">03 · COORDINATION</span><h3>Task handoffs</h3></div><span className="count">{tasks.length} TASKS</span></div>
              <form className="task-form" onSubmit={createTask}>
                <div><label htmlFor="task-title">Task title</label><input id="task-title" value={taskTitle} onChange={event => setTaskTitle(event.target.value)} placeholder="e.g. Review the API design" maxLength={160} required /></div>
                <div><label htmlFor="assignee">Assign to agent</label><select id="assignee" value={assignee} onChange={event => setAssignee(event.target.value)} required><option value="">Choose an agent</option>{agents.map(agent => <option value={agent.id} key={agent.id}>{agent.display_name} ({agent.provider})</option>)}</select></div>
                <div className="task-objective"><label htmlFor="task-objective">Expected outcome / instructions</label><textarea id="task-objective" value={taskObjective} onChange={event => setTaskObjective(event.target.value)} placeholder="Describe what the receiving agent should do and what evidence it should return…" maxLength={10000} rows={3} required /></div>
                <div className="context-picker"><span className="field-label">Attach context artifacts</span>{artifacts.length === 0 ? <small>No artifacts captured yet.</small> : artifacts.map(artifact => <label className="context-option" key={artifact.id}><input type="checkbox" checked={selectedArtifactIds.includes(artifact.id)} onChange={event => setSelectedArtifactIds(current => event.target.checked ? [...current, artifact.id] : current.filter(id => id !== artifact.id))} /><span>{artifact.title}</span></label>)}</div>
                <button disabled={!taskTitle.trim() || !taskObjective.trim() || !assignee || busy || agents.length === 0}>{busy ? "Creating…" : "Create handoff"}</button>
              </form>
              <div className="task-list">
                {tasks.map(task => {
                  const assigneeAgent = agents.find(agent => agent.id === task.assignee_agent_id);
                  const next = nextStatus[task.status];
                  return <article className="task-card" key={task.id}>
                    <div className="task-card-head"><div><span className="kicker">TASK · V{task.version}</span><h4>{task.title}</h4></div><span className={`task-status status-${task.status}`}>{task.status.replaceAll("_", " ")}</span></div>
                    <p>{task.objective}</p>
                    <div className="task-meta"><span>Assigned to <strong>{assigneeAgent?.display_name ?? "Unknown agent"}</strong></span><span>{task.transitions.length} events</span></div>
                    {task.status === "in_progress" && <div className="response-capture"><label htmlFor={`response-${task.id}`}>Capture the agent's response</label><textarea id={`response-${task.id}`} rows={4} value={responseDrafts[task.id] ?? ""} onChange={event => setResponseDrafts(current => ({ ...current, [task.id]: event.target.value }))} placeholder="Paste the response returned by the assigned assistant…" /><button className="advance-button" disabled={busy || !(responseDrafts[task.id] ?? "").trim()} onClick={() => void submitTaskResponse(task)}>Submit response as artifact →</button></div>}
                    {next && task.status !== "in_progress" && <button className="advance-button" disabled={busy} onClick={() => void advanceTask(task, next)}>Advance to {next.replaceAll("_", " ")} →</button>}
                    {task.status === "review_required" && <button className="request-changes-button" disabled={busy} onClick={() => void advanceTask(task, "changes_requested")}>Request changes</button>}
                    {task.response_artifact_id && <p className="response-linked">Response artifact linked · {task.response_artifact_id.slice(0, 8)}</p>}
                    <details className="history"><summary>View handoff history</summary>{task.transitions.map((transition, index) => <div className="history-row" key={index}><strong>{transition.to.replaceAll("_", " ")}</strong><span>{transition.note}</span><small>{new Date(transition.occurred_at).toLocaleString()}</small></div>)}</details>
                  </article>;
                })}
                {tasks.length === 0 && <div className="empty-state compact"><h3>No handoffs yet.</h3><p>Register an agent and create the first task above.</p></div>}
              </div>
            </section>
          </>
        )}

        {error && <div className="error" role="alert">{error} <button className="retry" onClick={() => selected ? void refreshWorkspace(selected) : void refreshWorkspaces()}>Retry</button></div>}
        <nav className="mobile-bottom-nav" aria-label="Primary navigation"><button type="button" className={activeView === "chats" ? "active" : ""} onClick={() => { setActiveView("chats"); setMobileConversationOpen(false); }}><span>◌</span>Chats{chats.reduce((total,chat)=>total+chat.unread_count,0)>0 && <span className="chat-unread-count">{chats.reduce((total,chat)=>total+chat.unread_count,0)}</span>}</button><button type="button" className={activeView === "explore" ? "active" : ""} onClick={() => { setActiveView("explore"); setMobileConversationOpen(false); }}><span>⌕</span>Explore</button></nav>
        {profileOpen && <div className="profile-overlay" role="dialog" aria-modal="true" aria-label="Profile and workspace overview"><div className="profile-overlay-header"><strong>Profile & workspace</strong><button type="button" onClick={() => setProfileOpen(false)}>Close</button></div><div className="profile-overlay-body"><div className="profile-identity"><div className="profile-avatar">{user.photoURL ? <img src={user.photoURL} alt="" /> : (user.displayName || user.email || "A").slice(0,1).toUpperCase()}</div><div className="profile-identity-copy"><strong>{user.displayName || user.email || "Axon user"}</strong><small>{selected?.name ?? "Personal workspace"}</small></div></div><div className="profile-stat-grid"><div><strong>{agents.length}</strong><small>Agents</small></div><div><strong>{chats.length}</strong><small>Chats</small></div><div><strong>{tasks.filter(task => task.status === "in_progress").length}</strong><small>Running</small></div><div><strong>{tasks.filter(task => task.status === "completed").length}</strong><small>Completed</small></div><div><strong>{tasks.filter(task => task.status === "offered" || task.status === "acknowledged").length}</strong><small>Queued</small></div><div><strong>{artifacts.length}</strong><small>Artifacts</small></div></div><p className="profile-overlay-email">{user.email}</p><button type="button" className="signout-button" onClick={() => { if (auth) void signOut(auth); }}>Sign out</button></div></div>}
        <footer><span>AXON</span><span>Context should travel with the work.</span><span>FOUNDATION BUILD · 0.1</span></footer>
        {health && health.storage !== "persistent" && (
          <p className="dev-warning">Development mode: data is temporary and this API has no authentication. Do not use sensitive or production data.</p>
        )}
      </section>
    </main>
  );
}


function AuthGate() {
  const [user, setUser] = useState<import("firebase/auth").User | null>(null);
  const [ready, setReady] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [mode, setMode] = useState<"sign_in" | "create">("sign_in");
  const [authError, setAuthError] = useState("");
  const [working, setWorking] = useState(false);

  useEffect(() => {
    if (!auth) { setReady(true); return; }
    return onAuthStateChanged(auth, current => { setUser(current); setReady(true); });
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!auth) return;
    setWorking(true);
    setAuthError("");
    try {
      if (mode === "create") await createUserWithEmailAndPassword(auth, email.trim(), password);
      else await signInWithEmailAndPassword(auth, email.trim(), password);
    } catch (cause) {
      setAuthError(cause instanceof Error ? cause.message : "Authentication failed.");
    } finally { setWorking(false); }
  }

  if (!ready) return <main className="shell"><section className="content"><p>Restoring your secure session…</p></section></main>;
  if (!auth) return <main className="shell"><section className="content"><h1>Firebase configuration required</h1><p>Set the VITE_FIREBASE_* environment variables for this web app, then restart it.</p></section></main>;
  if (user) return <WorkspaceApp user={user} />;
  return <main className="shell"><section className="content" style={{maxWidth:520,margin:"12vh auto"}}><p className="eyebrow">AXON · SECURE ACCESS</p><h1>{mode === "create" ? "Create your account" : "Welcome back"}</h1><p>Sign in to access your personal workspace and its data.</p><form className="stack-form" onSubmit={submit}><label htmlFor="auth-email">Email</label><input id="auth-email" type="email" autoComplete="email" required value={email} onChange={e=>setEmail(e.target.value)}/><label htmlFor="auth-password">Password</label><input id="auth-password" type="password" minLength={6} autoComplete={mode === "create" ? "new-password" : "current-password"} required value={password} onChange={e=>setPassword(e.target.value)}/><button disabled={working}>{working ? "Please wait…" : mode === "create" ? "Create account" : "Sign in"}</button></form>{authError && <p role="alert">{authError}</p>}<button className="back-link" onClick={()=>setMode(mode === "create" ? "sign_in" : "create")}>{mode === "create" ? "Already registered? Sign in" : "New to Axon? Create an account"}</button></section></main>;
}

import { onAuthStateChanged, signInWithEmailAndPassword, createUserWithEmailAndPassword, signOut } from "firebase/auth";

export default function App() { return <AuthGate />; }
