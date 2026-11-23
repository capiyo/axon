import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { auth } from "./firebase";

import type {
  Agent, Artifact, Health, Task, Workspace, Chat, ChatDetail, ChatMessage,
  ProjectMemoryEntry, QoinVectorSnapshot, HandoverContext, ProgressiveMemoryContext,
  QoinMemoryIndexReceipt, QoinChatMessageIndexReceipt
} from "../../../packages/contracts/src";


const API_BASE_URL = (import.meta.env.VITE_AXON_API_URL ?? "").replace(/\/$/, "");

const api = async <T,>(path: string, init?: RequestInit): Promise<T> => {
  const token = await auth?.currentUser?.getIdToken();
  const response = await fetch(`${API_BASE_URL}${path}`, {
    ...init,
    headers: { "Content-Type": "application/json", ...(token ? { Authorization: `Bearer ${token}` } : {}), ...init?.headers }
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }));
    throw new Error(body.error ?? "Request failed");
  }
  return response.json() as Promise<T>;
};

function WorkspaceApp() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [selected, setSelected] = useState<Workspace | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [chats, setChats] = useState<Chat[]>([]);
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
  const [activeTab, setActiveTab] = useState<"chats" | "explore">("chats");
  const [menuOpen, setMenuOpen] = useState(false);
  const messageEndRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    messageEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [chatDetail?.messages.length, activeChat]);

  async function refreshWorkspaces() {
    const [items, status] = await Promise.all([
      api<Workspace[]>("/api/v1/workspaces"),
      api<Health>("/health")
    ]);
    setWorkspaces(items);
    setSelected(current => current ?? items[0] ?? null);
    setHealth(status);
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
    const recentChats = [...chatItems].sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at));
    setChats(recentChats);
    setActiveChat(current => current && recentChats.some(chat => chat.id === current) ? current : recentChats[0]?.id ?? "");
    setAssignee(current => agentItems.some(agent => agent.id === current) ? current : agentItems[0]?.id ?? "");
    setHandoverAgentId(current => agentItems.some(agent => agent.id === current) ? current : agentItems[0]?.id ?? "");
  }

  useEffect(() => {
    void refreshWorkspaces().catch(cause => setError(cause instanceof Error ? cause.message : "Could not reach the Axon API."));
  }, []);

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
    api<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`)
      .then(setChatDetail)
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
      setActiveChat("");
      setChatDetail(null);
      setChats(await api<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`));
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not archive chat."); }
    finally { setBusy(false); }
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

  const currentUser = auth?.currentUser;
  const profileName = currentUser?.displayName || currentUser?.email?.split("@")[0] || "Your profile";
  const avatarLetter = profileName.slice(0, 1).toUpperCase();

  return (
    <main className="axon-app">
      <header className="axon-app-header">
        <div className="axon-brand"><span className="axon-brand-mark">A</span><span>AXON</span></div>
        <div className="axon-header-actions">
          <span className={health?.status === "ok" ? "axon-connection online" : "axon-connection"}><i />{health?.status === "ok" ? "Connected" : "Offline"}</span>
          <button type="button" className="axon-icon-button" aria-label="Open app menu" onClick={() => setMenuOpen(value => !value)}>☰</button>
          <button type="button" className="axon-profile-button" aria-label="Profile menu" onClick={() => setMenuOpen(value => !value)}>{currentUser?.photoURL ? <img src={currentUser.photoURL} alt="" /> : avatarLetter}</button>
        </div>
        {menuOpen && <div className="axon-menu">
          <div className="axon-menu-profile">
            <div className="axon-menu-avatar">{currentUser?.photoURL ? <img src={currentUser.photoURL} alt="" /> : avatarLetter}</div>
            <div><strong>{profileName}</strong><small>{currentUser?.email ?? "Axon member"}</small><span><i /> Available</span></div>
          </div>
          <div className="axon-menu-workspace"><span className="axon-section-label">CURRENT WORKSPACE</span><strong>{selected?.name ?? "No workspace selected"}</strong></div>
          <button type="button" onClick={() => { setActiveTab("explore"); setMenuOpen(false); }}>Explore workspace</button>
          <button type="button" onClick={() => { setSelected(null); setMenuOpen(false); }}>Switch workspace</button>
          <button type="button" onClick={() => { const firebaseAuth = auth; if (firebaseAuth) void import("firebase/auth").then(({signOut}) => signOut(firebaseAuth)); setMenuOpen(false); }}>Sign out</button>
        </div>}
      </header>

      <nav className="axon-mobile-tabs" aria-label="Main navigation">
        <button type="button" className={activeTab === "chats" ? "active" : ""} onClick={() => setActiveTab("chats")}><span>◉</span>Chats</button>
        <button type="button" className={activeTab === "explore" ? "active" : ""} onClick={() => setActiveTab("explore")}><span>⌕</span>Explore</button>
      </nav>

      <section className="axon-desktop-layout">
        <aside className="axon-profile-pane">
          <div className="axon-profile-card">
            <div className="axon-avatar">{currentUser?.photoURL ? <img src={currentUser.photoURL} alt="" /> : avatarLetter}</div>
            <strong>{profileName}</strong><small>{currentUser?.email ?? "Axon member"}</small>
            <span className="axon-presence"><i /> Available</span>
          </div>
          <div className="axon-pane-section"><span className="axon-section-label">WORKSPACE</span>
            {selected ? <button type="button" className="axon-workspace-choice active" onClick={() => setActiveTab("explore")}><span className="axon-workspace-glyph">{selected.name.slice(0,1).toUpperCase()}</span><span><strong>{selected.name}</strong><small>Personal workspace</small></span></button> : <p className="muted">Choose a workspace in Explore.</p>}
          </div>
          <div className="axon-profile-footer"><span className={health?.status === "ok" ? "axon-health-dot online" : "axon-health-dot"} />{health?.status === "ok" ? "Axon is connected" : "Connecting to Axon"}</div>
        </aside>

        <aside className={"axon-chats-pane" + (activeTab === "explore" ? " mobile-hidden" : "")}>
          <div className="axon-pane-heading"><div><span className="axon-section-label">YOUR CONVERSATIONS</span><strong>Chats</strong></div><span className="axon-chat-count">{chats.length}</span></div>
          <form className="axon-new-chat" onSubmit={createChat}><input aria-label="New chat title" value={chatTitle} onChange={event => setChatTitle(event.target.value)} placeholder="Start a new chat…" maxLength={160} required /><button disabled={!chatTitle.trim() || busy} aria-label="Create chat">＋</button></form>
          <div className="axon-chat-list">
            {chats.map(chat => {
              const isActive = activeChat === chat.id;
              const latestMessage = isActive && chatDetail?.chat.id === chat.id ? chatDetail.messages[chatDetail.messages.length - 1] : undefined;
              const messageCount = isActive && chatDetail?.chat.id === chat.id ? chatDetail.messages.length : null;
              return <button type="button" key={chat.id} className={isActive ? "axon-chat-item active" : "axon-chat-item"} onClick={() => { setActiveChat(chat.id); setActiveTab("chats"); }} aria-label={`Open conversation: ${chat.title}`}>
                <span className="axon-chat-meta"><span className="axon-chat-kind">WORKSPACE CHAT</span><time dateTime={chat.updated_at}>{new Date(chat.updated_at).toLocaleDateString([], { month: "short", day: "numeric" })}</time></span>
                <span className="axon-chat-title-row"><span className="axon-chat-avatar">{chat.title.slice(0,1).toUpperCase()}</span><strong>{chat.title}</strong><span className="axon-chat-chevron">›</span></span>
                <span className="axon-chat-preview">{latestMessage ? `${latestMessage.role === "user" ? "You" : latestMessage.answered_by || "Agent"}: ${latestMessage.content}` : "Open this conversation to continue your work."}</span>
                <span className="axon-chat-footer"><span>{selected?.name ?? "Personal workspace"}</span><span>{messageCount === null ? "Private conversation" : `${messageCount} messages`}</span></span>
              </button>;
            })}
            {chats.length === 0 && <div className="axon-empty-list"><span>◌</span><strong>No conversations yet</strong><small>Create a chat to begin working with your agents.</small></div>}
          </div>
        </aside>

        <section className={"axon-conversation-pane" + (activeTab === "explore" ? " mobile-hidden" : "")}>
          {!selected ? <div className="axon-empty-conversation"><span>✳</span><strong>Your workspace is ready when you are.</strong><p>Create or select a workspace from Explore to start a conversation.</p><button type="button" onClick={() => setActiveTab("explore")}>Explore workspaces</button></div> : !chatDetail ? <div className="axon-empty-conversation"><span>✳</span><strong>Start a conversation</strong><p>Select a chat or create a new one. Your most recent conversation will open here.</p></div> : <>
            <div className="axon-conversation-header"><div className="axon-conversation-avatar">{chatDetail.chat.title.slice(0,1).toUpperCase()}</div><div><strong>{chatDetail.chat.title}</strong><small>{selected.name} · private workspace chat</small></div><button type="button" className="axon-more-button" onClick={() => void archiveChat()} disabled={busy} title="Archive conversation">•••</button></div>
            <div className="axon-resume-brief"><span>CONTEXT BRIEF</span><p>{chatDetail.resume_brief || "Useful context approved in this conversation will appear here for future handovers."}</p></div>
            <div className="axon-message-stream">
              {chatDetail.messages.map(message => <article key={message.id} className={"axon-message-row " + (message.role === "user" ? "mine" : "theirs")}>
                <div className="axon-message-avatar">{message.role === "user" ? avatarLetter : (message.answered_by || message.role).slice(0,1).toUpperCase()}</div>
                <div className="axon-message-content"><div className="axon-message-meta"><strong>{message.role === "user" ? "You" : message.answered_by || message.role}</strong><span>{new Date(message.created_at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</span></div><div className="axon-message-bubble"><p>{message.content}</p></div><button type="button" className="axon-message-action" disabled={busy} onClick={() => void indexChatMessageWithQoin(message)}>Index with Qoin</button></div>
              </article>)}
              <div ref={messageEndRef} />
              {chatDetail.messages.length === 0 && <div className="axon-empty-stream"><span>✧</span><strong>This conversation is new</strong><p>Send a message to start the working history.</p></div>}
            </div>
            <form className="axon-composer" onSubmit={sendChatMessage}><textarea value={messageDraft} onChange={event => setMessageDraft(event.target.value)} rows={2} maxLength={256000} placeholder="Message your workspace…" required /><div className="axon-composer-footer"><span>Messages become part of this workspace's history.</span><button disabled={!messageDraft.trim() || busy}>{busy ? "Sending…" : "Send"} <span>↗</span></button></div></form>
            <details className="axon-memory-drawer"><summary>Project memory <span>{chatDetail.memory.length} entries</span></summary>
              <form className="axon-memory-form" onSubmit={proposeMemory}><select value={memoryType} onChange={event => setMemoryType(event.target.value as ProjectMemoryEntry["type"])}><option value="note">Note</option><option value="decision">Decision</option><option value="fact">Fact</option><option value="constraint">Constraint</option><option value="open_task">Open task</option></select><input value={memoryDraft} onChange={event => setMemoryDraft(event.target.value)} placeholder="Propose a useful memory…" maxLength={4000} required /><button disabled={!memoryDraft.trim() || busy}>Add</button></form>
              {chatDetail.memory.map(entry => <div className="axon-memory-entry" key={entry.id}><div><strong>{entry.type.replaceAll("_"," ")}</strong><small>{entry.status}</small></div>{editingMemoryId === entry.id ? <form className="axon-memory-form" onSubmit={event => { event.preventDefault(); void saveMemoryEdit(entry); }}><input value={editMemoryDraft} onChange={event => setEditMemoryDraft(event.target.value)} maxLength={4000} required aria-label="Edit memory text" /><button disabled={!editMemoryDraft.trim() || busy}>Save</button><button type="button" className="axon-memory-cancel" onClick={() => { setEditingMemoryId(""); setEditMemoryDraft(""); }}>Cancel</button></form> : <><p>{entry.text}</p><div className="axon-memory-actions"><button type="button" onClick={() => { setEditingMemoryId(entry.id); setEditMemoryDraft(entry.text); }}>Edit</button><button type="button" onClick={() => void indexMemoryWithQoin(entry)} disabled={busy}>Index with Qoin</button>{entry.status === "proposed" && <><button type="button" onClick={() => void reviewMemory(entry,"approve")} disabled={busy}>Approve</button><button type="button" onClick={() => void reviewMemory(entry,"reject")} disabled={busy}>Reject</button></>}</div></>}</div>)}
            </details>
          </>}
        </section>

        <section className={"axon-explore-pane" + (activeTab === "explore" ? " active" : "")}>
          <div className="axon-explore-heading"><span className="axon-section-label">DISCOVER YOUR WORKSPACE</span><strong>Explore</strong><p>Manage agents, context and handoffs across your projects.</p></div>
          <div className="axon-workspace-switcher">
            <span className="axon-section-label">WORKSPACES</span>
            <form onSubmit={createWorkspace}><input id="workspace-name" value={name} onChange={event => setName(event.target.value)} placeholder="New workspace name" maxLength={120} required /><button disabled={!name.trim() || busy}>＋</button></form>
            <div className="axon-workspace-list">{workspaces.map(workspace => <button type="button" key={workspace.id} className={selected?.id === workspace.id ? "active" : ""} onClick={() => { setSelected(workspace); setActiveTab("explore"); }}><span className="axon-workspace-glyph">{workspace.name.slice(0,1).toUpperCase()}</span><span><strong>{workspace.name}</strong><small>{new Date(workspace.created_at).toLocaleDateString()}</small></span><span>›</span></button>)}</div>
          </div>
          {selected ? <div className="axon-explore-tools">            <section className="panel qoin-memory-panel">
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
            </section></div> : <div className="axon-empty-explore">Select a workspace to manage its agents, artifacts, Qoin context and task handoffs.</div>}
        </section>
      </section>
      {error && <div className="axon-error" role="alert">{error}<button type="button" onClick={() => selected ? void refreshWorkspace(selected) : void refreshWorkspaces()}>Retry</button></div>}
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
  if (user) return <><div style={{position:"fixed",right:24,top:18,zIndex:10,display:"flex",gap:12,alignItems:"center",fontSize:12}}><span>{user.email}</span><button onClick={() => { if (auth) void signOut(auth); }}>Sign out</button></div><WorkspaceApp /></>;
  return <main className="shell"><section className="content" style={{maxWidth:520,margin:"12vh auto"}}><p className="eyebrow">AXON · SECURE ACCESS</p><h1>{mode === "create" ? "Create your account" : "Welcome back"}</h1><p>Sign in to access your personal workspace and its data.</p><form className="stack-form" onSubmit={submit}><label htmlFor="auth-email">Email</label><input id="auth-email" type="email" autoComplete="email" required value={email} onChange={e=>setEmail(e.target.value)}/><label htmlFor="auth-password">Password</label><input id="auth-password" type="password" minLength={6} autoComplete={mode === "create" ? "new-password" : "current-password"} required value={password} onChange={e=>setPassword(e.target.value)}/><button disabled={working}>{working ? "Please wait…" : mode === "create" ? "Create account" : "Sign in"}</button></form>{authError && <p role="alert">{authError}</p>}<button className="back-link" onClick={()=>setMode(mode === "create" ? "sign_in" : "create")}>{mode === "create" ? "Already registered? Sign in" : "New to Axon? Create an account"}</button></section></main>;
}

import { onAuthStateChanged, signInWithEmailAndPassword, createUserWithEmailAndPassword, signOut } from "firebase/auth";

export default function App() { return <AuthGate />; }
