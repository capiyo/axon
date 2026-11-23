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

import type { Agent, Artifact, Health, Task, Workspace, Chat, ChatDetail, ChatMessage, ProjectMemoryEntry, QoinVectorSnapshot, HandoverContext, ProgressiveMemoryContext, QoinMemoryIndexReceipt, QoinChatMessageIndexReceipt } from "../../packages/contracts/src";


const API_BASE_URL = process.env.EXPO_PUBLIC_AXON_API_URL ?? "http://127.0.0.1:8080";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
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

function WorkspaceApp() {
  const [health, setHealth] = useState<Health | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [selected, setSelected] = useState<Workspace | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [chats, setChats] = useState<Chat[]>([]);
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
  const [activeTab, setActiveTab] = useState<"chats" | "explore">("chats");
  const [menuOpen, setMenuOpen] = useState(false);

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
    if (!agentItems.some(agent => agent.id === assignee)) setAssignee(agentItems[0]?.id ?? "");
    if (!agentItems.some(agent => agent.id === handoverAgentId)) setHandoverAgentId(agentItems[0]?.id ?? "");
  }

  useEffect(() => {
    refreshHome().catch(cause => setError(cause instanceof Error ? cause.message : "API unavailable"));
  }, []);

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
    if (!selected || !activeChat) { setChatDetail(null); return; }
    request<ChatDetail>(`/api/v1/workspaces/${selected.id}/chats/${activeChat}`)
      .then(setChatDetail)
      .catch(cause => setError(cause instanceof Error ? cause.message : "Could not load chat"));
  }, [selected?.id, activeChat]);

  function createChat() {
    if (!selected || !chatTitle.trim()) return;
    void perform(async () => {
      const chat = await request<Chat>(`/api/v1/workspaces/${selected.id}/chats`, {
        method: "POST", body: JSON.stringify({ title: chatTitle.trim() })
      });
      setChatTitle("");
      setChats(current => [chat, ...current]);
      setActiveChat(chat.id);
    });
  }

  function archiveChat() {
    if (!selected || !activeChat) return;
    void perform(async () => {
      await request(`/api/v1/workspaces/${selected.id}/chats/${activeChat}/archive`, { method: "PATCH" });
      setActiveChat("");
      setChatDetail(null);
      setChats(await request<Chat[]>(`/api/v1/workspaces/${selected.id}/chats`));
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

  const user = auth?.currentUser;
  const displayName = user?.displayName || user?.email?.split("@")[0] || "Your profile";
  const initial = displayName.slice(0,1).toUpperCase();

  return (
    <SafeAreaView style={[styles.safe,{backgroundColor:"#0e1612"}]}>
      <StatusBar style="light" />
      <View style={{height:58,paddingHorizontal:16,flexDirection:"row",alignItems:"center",justifyContent:"space-between",backgroundColor:"#0e1612",borderBottomWidth:1,borderBottomColor:"#1d2a20"}}>
        <View style={{flexDirection:"row",alignItems:"center",gap:10}}>
          <View style={{width:34,height:34,borderRadius:12,backgroundColor:"#bfff4b",alignItems:"center",justifyContent:"center"}}><Text style={{color:"#17200f",fontSize:18,fontWeight:"900"}}>A</Text></View>
          <Text style={{color:"#e8eee9",fontSize:13,fontWeight:"900",letterSpacing:2}}>AXON</Text>
        </View>
        <View style={{flexDirection:"row",alignItems:"center",gap:14}}>
          <Pressable onPress={()=>setMenuOpen(value=>!value)} accessibilityLabel="Open app menu"><Text style={{color:"#e8eee9",fontSize:23}}>☰</Text></Pressable>
          <Pressable onPress={()=>setMenuOpen(value=>!value)} style={{width:34,height:34,borderRadius:17,backgroundColor:"#29372b",alignItems:"center",justifyContent:"center",overflow:"hidden"}} accessibilityLabel="Open profile">
            {user?.photoURL ? <Image source={{uri:user.photoURL}} style={{width:34,height:34,borderRadius:17}} /> : <Text style={{color:"#c5f58b",fontWeight:"900"}}>{initial}</Text>}
          </Pressable>
        </View>
      </View>

      {menuOpen ? <View style={{position:"absolute",top:62,right:12,zIndex:20,width:245,padding:14,borderRadius:15,backgroundColor:"#17221a",borderWidth:1,borderColor:"#2a392c"}}>
        <Text style={{color:"#e8eee9",fontSize:13,fontWeight:"800"}}>{displayName}</Text>
        <Text style={{color:"#87948a",fontSize:10,marginTop:4,marginBottom:12}}>{user?.email ?? "Axon member"}</Text>
        <Text style={{color:"#87948a",fontSize:9,letterSpacing:1,marginBottom:7}}>SWITCH WORKSPACE</Text>
        {workspaces.map(workspace=><Pressable key={workspace.id} onPress={()=>{setSelected(workspace);setMenuOpen(false);setActiveTab("chats");}} style={{paddingVertical:9,flexDirection:"row",alignItems:"center",gap:9}}><Text style={{color:"#bfff4b",fontWeight:"800"}}>{workspace.name.slice(0,1).toUpperCase()}</Text><Text numberOfLines={1} style={{color:"#e8eee9",fontSize:11,flex:1}}>{workspace.name}</Text>{selected?.id===workspace.id?<Text style={{color:"#bfff4b"}}>✓</Text>:null}</Pressable>)}
        <Pressable onPress={()=>{setMenuOpen(false);if(auth)void signOut(auth);}} style={{paddingTop:12,borderTopWidth:1,borderTopColor:"#2a392c",marginTop:5}}><Text style={{color:"#e6b0a7",fontSize:11,fontWeight:"700"}}>Sign out</Text></Pressable>
      </View> : null}

      <View style={{flex:1,backgroundColor:"#0e1612"}}>
        {activeTab === "chats" ? <ScrollView contentContainerStyle={{padding:15,paddingBottom:112}} keyboardShouldPersistTaps="handled">
          <View style={{flexDirection:"row",alignItems:"center",justifyContent:"space-between",marginBottom:12}}>
            <View><Text style={{color:"#849087",fontSize:9,letterSpacing:1.2,fontWeight:"700"}}>WORKSPACE</Text><Text style={{color:"#e8eee9",fontSize:12,fontWeight:"700",marginTop:4}}>{selected?.name ?? "Select a workspace"}</Text></View>
            <View style={{width:7,height:7,borderRadius:4,backgroundColor:health?.status==="ok"?"#bfff4b":"#8a6c46"}}/>
          </View>
          {selected ? <>
            <View style={{flexDirection:"row",gap:8,marginBottom:12}}>
              <TextInput value={chatTitle} onChangeText={setChatTitle} placeholder="Start a new chat…" placeholderTextColor="#718073" style={{flex:1,backgroundColor:"#17221a",borderWidth:1,borderColor:"#29372b",borderRadius:12,paddingHorizontal:13,paddingVertical:11,color:"#e8eee9",fontSize:12}} maxLength={160}/>
              <Pressable onPress={createChat} disabled={!chatTitle.trim()||busy} style={{width:44,borderRadius:12,alignItems:"center",justifyContent:"center",backgroundColor:"#bfff4b",opacity:!chatTitle.trim()||busy?0.45:1}}><Text style={{color:"#17200f",fontSize:23,fontWeight:"800"}}>＋</Text></Pressable>
            </View>
            <View style={{gap:6,marginBottom:14}}>
              {chats.map(chat=><Pressable key={chat.id} onPress={()=>setActiveChat(chat.id)} style={{flexDirection:"row",alignItems:"center",gap:10,padding:11,borderRadius:13,backgroundColor:activeChat===chat.id?"#1b2a1d":"#141f17",borderWidth:1,borderColor:activeChat===chat.id?"#455c31":"#202d22"}}>
                <View style={{width:35,height:35,borderRadius:12,backgroundColor:"#29372b",alignItems:"center",justifyContent:"center"}}><Text style={{color:"#c5f58b",fontWeight:"800"}}>{chat.title.slice(0,1).toUpperCase()}</Text></View>
                <View style={{flex:1,minWidth:0}}><Text numberOfLines={1} style={{color:"#e8eee9",fontSize:12,fontWeight:"700"}}>{chat.title}</Text><Text style={{color:"#87948a",fontSize:9,marginTop:4}}>{new Date(chat.updated_at).toLocaleDateString()}</Text></View>
                <Text style={{color:"#718073",fontSize:20}}>›</Text>
              </Pressable>)}
              {chats.length===0?<View style={{padding:22,alignItems:"center",borderRadius:14,backgroundColor:"#141f17"}}><Text style={{color:"#bfff4b",fontSize:24}}>◌</Text><Text style={{color:"#e8eee9",fontSize:12,fontWeight:"700",marginTop:7}}>No conversations yet</Text><Text style={{color:"#87948a",fontSize:10,textAlign:"center",marginTop:5}}>Create a chat to start a shared working history.</Text></View>:null}
            </View>
            {chatDetail ? <>
              <View style={{flexDirection:"row",alignItems:"center",gap:10,paddingVertical:12,borderTopWidth:1,borderTopColor:"#263329",borderBottomWidth:1,borderBottomColor:"#263329"}}>
                <View style={{width:37,height:37,borderRadius:19,backgroundColor:"#29372b",alignItems:"center",justifyContent:"center"}}><Text style={{color:"#c5f58b",fontWeight:"800"}}>{chatDetail.chat.title.slice(0,1).toUpperCase()}</Text></View>
                <View style={{flex:1}}><Text style={{color:"#e8eee9",fontSize:13,fontWeight:"800"}}>{chatDetail.chat.title}</Text><Text style={{color:"#87948a",fontSize:9,marginTop:3}}>Private workspace conversation</Text></View>
                <Pressable onPress={archiveChat} disabled={busy}><Text style={{color:"#9dbb79",fontSize:10}}>Archive</Text></Pressable>
              </View>
              <View style={{padding:12,marginTop:12,backgroundColor:"#19251b",borderRadius:12,borderWidth:1,borderColor:"#2a392c"}}>
                <Text style={{color:"#a7d96b",fontSize:9,fontWeight:"800",letterSpacing:1}}>CONTEXT BRIEF</Text>
                <Text style={{color:"#b7c2b5",fontSize:10,lineHeight:16,marginTop:6}}>{chatDetail.resume_brief||"Approved project memory will shape future agent handovers."}</Text>
              </View>
              <View style={{gap:14,paddingVertical:16}}>
                {chatDetail.messages.map(message=><View key={message.id} style={{alignSelf:message.role==="user"?"flex-end":"flex-start",maxWidth:"88%",flexDirection:"row",gap:8}}>
                  {message.role!=="user"?<View style={{width:27,height:27,borderRadius:14,backgroundColor:"#29372b",alignItems:"center",justifyContent:"center"}}><Text style={{color:"#bfff4b",fontSize:10,fontWeight:"800"}}>{(message.answered_by||"A").slice(0,1).toUpperCase()}</Text></View>:null}
                  <View style={{flexShrink:1}}>
                    <Text style={{color:"#a0ad9f",fontSize:9,marginBottom:5,textAlign:message.role==="user"?"right":"left"}}>{message.role==="user"?"You":message.answered_by||"Assistant"}</Text>
                    <View style={{padding:12,borderRadius:15,backgroundColor:message.role==="user"?"#293b20":"#19241c",borderWidth:1,borderColor:"#2b392c"}}><Text style={{color:"#e1e9df",fontSize:12,lineHeight:19}}>{message.content}</Text></View>
                    <Pressable onPress={()=>indexChatMessageWithQoin(message)} disabled={busy} style={{alignSelf:message.role==="user"?"flex-end":"flex-start",paddingTop:5}}><Text style={{color:"#94b66b",fontSize:9}}>Index with Qoin</Text></Pressable>
                  </View>
                </View>)}
                {chatDetail.messages.length===0?<Text style={{color:"#87948a",fontSize:11,textAlign:"center",padding:20}}>This conversation is new. Send a message to begin.</Text>:null}
              </View>
              <View style={{padding:12,borderRadius:15,backgroundColor:"#17221a",borderWidth:1,borderColor:"#2a392c"}}>
                <TextInput value={messageDraft} onChangeText={setMessageDraft} placeholder="Message your workspace…" placeholderTextColor="#718073" multiline style={{minHeight:58,maxHeight:150,color:"#e8eee9",fontSize:12,textAlignVertical:"top"}} maxLength={256000}/>
                <View style={{flexDirection:"row",alignItems:"center",justifyContent:"space-between",marginTop:7}}><Text style={{color:"#718073",fontSize:9,flex:1}}>Messages stay in this workspace.</Text><Pressable onPress={sendChatMessage} disabled={!messageDraft.trim()||busy} style={{paddingHorizontal:17,paddingVertical:10,borderRadius:10,backgroundColor:"#bfff4b",opacity:!messageDraft.trim()||busy?0.45:1}}><Text style={{color:"#17200f",fontWeight:"900",fontSize:10}}>{busy?"Sending…":"Send ↗"}</Text></Pressable></View>
              </View>
              <View style={{marginTop:14,padding:13,borderRadius:13,backgroundColor:"#141f17"}}>
                <Text style={{color:"#e8eee9",fontSize:11,fontWeight:"800",marginBottom:9}}>Project memory · {chatDetail.memory.length}</Text>
                <TextInput value={memoryDraft} onChangeText={setMemoryDraft} placeholder="Propose a decision, fact or note…" placeholderTextColor="#718073" style={{borderWidth:1,borderColor:"#29372b",borderRadius:9,padding:10,color:"#e8eee9",fontSize:11}}/>
                <ActionButton label="Propose memory" onPress={proposeMemory} disabled={!memoryDraft.trim()||busy}/>
                {chatDetail.memory.map(entry=><View key={entry.id} style={{paddingVertical:9,borderTopWidth:1,borderTopColor:"#29372b",marginTop:8}}><Text style={{color:"#c5f58b",fontSize:10,fontWeight:"800"}}>{entry.type.replaceAll("_"," ")} · {entry.status}</Text><Text style={{color:"#c3cec0",fontSize:10,lineHeight:15,marginTop:5}}>{entry.text}</Text>{entry.status==="proposed"?<View style={{flexDirection:"row",gap:8}}><ActionButton label="Approve" onPress={()=>approveMemory(entry)} secondary/><ActionButton label="Reject" onPress={()=>changeMemory(entry,"reject")} secondary/></View>:null}</View>)}
              </View>
            </> : <Text style={{color:"#87948a",fontSize:11,textAlign:"center",padding:18}}>Choose a conversation above to open messaging.</Text>}
          </> : <View style={{padding:20,backgroundColor:"#17221a",borderRadius:15}}><Text style={{color:"#e8eee9",fontSize:13,fontWeight:"800"}}>Your chats are waiting</Text><Text style={{color:"#87948a",fontSize:11,lineHeight:17,marginTop:7}}>Choose or create a workspace in Explore to start.</Text><ActionButton label="Explore workspaces" onPress={()=>setActiveTab("explore")}/></View>}
        </ScrollView> : <ScrollView contentContainerStyle={{padding:15,paddingBottom:112}} keyboardShouldPersistTaps="handled">
          <Text style={{color:"#849087",fontSize:9,letterSpacing:1.2,fontWeight:"800",marginBottom:10}}>YOUR WORKSPACE</Text>
          <View style={{padding:13,backgroundColor:"#17221a",borderRadius:14,marginBottom:12}}>
            <Text style={{color:"#e8eee9",fontSize:12,fontWeight:"800"}}>{selected?.name??"Create a workspace"}</Text>
            <Text style={{color:"#87948a",fontSize:10,marginTop:5}}>Agents, context, Qoin memory and task handoffs.</Text>
          </View>
          {selected ? <>
            <View style={{padding:14,backgroundColor:"#141f17",borderRadius:14,marginBottom:12}}>
              <Text style={{color:"#a7d96b",fontSize:9,fontWeight:"800",letterSpacing:1}}>WORKSPACE TOOLS</Text>
              <Text style={{color:"#e8eee9",fontSize:13,fontWeight:"800",marginTop:6}}>Agents & coordination</Text>
            </View>
                        <View style={styles.panel}>
              <Text style={styles.panelTitle}>05 · Qoin × Axon · Market-aware memory</Text>
              <Text style={styles.muted}>Qoin supplies its six-dimensional attended market-state vector. Axon uses it to rank historical memories; this is a market-state vector, not a text embedding.</Text>
              <TextInput value={qoinSymbol} onChangeText={value => { setQoinSymbol(value.toUpperCase()); setQoinVector(null); setHandover(null); setProgressiveContext(null); }} placeholder="Market symbol (BTCUSDT)" autoCapitalize="characters" style={styles.input} />
              <ActionButton label="Load latest Qoin vector" onPress={loadQoinVector} disabled={busy || !qoinSymbol.trim()} />
              {qoinNotice ? <Text style={styles.responseNote}>{qoinNotice}</Text> : null}
              {qoinVector ? <>
                <Text style={styles.rowTitle}>{qoinVector.symbol} · {qoinVector.vector.model_version} · {qoinVector.action.toUpperCase()}</Text>
                {Object.entries(qoinVector.features).map(([name, value]) => <Text key={name} style={styles.rowMeta}>{name.replaceAll("_", " ")}: {value.toFixed(3)}</Text>)}
                <Text style={styles.rowMeta}>{qoinVector.reason}</Text>
              </> : <Text style={styles.muted}>Load a current Qoin vector to index approved memories and prepare agent context.</Text>}
              <Text style={styles.fieldLabel}>RECEIVING AGENT</Text>
              {agents.map(agent => <Pressable key={agent.id} onPress={() => { setHandoverAgentId(agent.id); setHandover(null); setProgressiveContext(null); }} style={[styles.agentChoice, handoverAgentId === agent.id && styles.agentChoiceActive]}><Text style={styles.rowTitle}>{agent.display_name} · {agent.provider}</Text></Pressable>)}
              <ActionButton label="Prepare first 20% context" onPress={prepareAgentHandover} disabled={busy || !qoinVector || !handoverAgentId || agents.length === 0} />
              {handover ? <>
                <Text style={styles.fieldLabel}>PINNED EPOCH {handover.epoch.epoch_number} · {handover.initial.length} INITIAL MEMORIES</Text>
                <Text style={styles.rowTitle}>{(handover.achieved_coverage * 100).toFixed(1)}% measured contribution coverage</Text>
                <Text style={styles.rowMeta}>{handover.deferred_count} memories deferred{handover.budget_exhausted ? " · 20% budget reached before 80% target" : ""}</Text>
                {handover.initial.map(item => <View key={item.memory_id} style={styles.listRow}><Text style={styles.rowTitle}>{item.source_kind} · {(item.attention_score * 100).toFixed(1)}% attention</Text><Text style={styles.rowMeta}>{item.payload_json}</Text></View>)}
                <ActionButton label="Retrieve next 10 historical memories" onPress={retrieveMoreContext} disabled={busy || handover.deferred_count === 0} secondary />
                {progressiveContext ? <><Text style={styles.fieldLabel}>ADDITIONAL CONTEXT · {progressiveContext.memories.length} ADDED · {progressiveContext.remaining_candidates} LEFT</Text>{progressiveContext.memories.map(item => <View key={item.memory_id} style={styles.listRow}><Text style={styles.rowTitle}>{item.source_kind} · {(item.attention_score * 100).toFixed(1)}% attention</Text><Text style={styles.rowMeta}>{item.payload_json}</Text></View>)}</> : null}
              </> : null}
            </View>

            <View style={styles.panel}>
              <Text style={styles.panelTitle}>01 · Agents ({agents.length})</Text>
              <TextInput value={agentName} onChangeText={setAgentName} placeholder="Agent name" style={styles.input} />
              <TextInput value={provider} onChangeText={setProvider} placeholder="Provider (claude, chatgpt…)" style={styles.input} autoCapitalize="none" />
              <ActionButton label="Register agent" onPress={registerAgent} disabled={!agentName.trim() || busy} />
              {agents.map(agent => <View key={agent.id} style={styles.listRow}><Text style={styles.rowTitle}>{agent.display_name}</Text><Text style={styles.rowMeta}>{agent.provider} · manual</Text></View>)}
              {agents.length === 0 && <Text style={styles.muted}>No agents registered yet.</Text>}
            </View>

            <View style={styles.panel}>
              <Text style={styles.panelTitle}>02 · Capture context ({artifacts.length})</Text>
              <TextInput value={artifactTitle} onChangeText={setArtifactTitle} placeholder="Artifact title" style={styles.input} />
              <TextInput value={artifactContent} onChangeText={setArtifactContent} placeholder="Paste findings, responses or project context…" style={[styles.input, styles.multiline]} multiline textAlignVertical="top" />
              <ActionButton label="Capture artifact" onPress={captureArtifact} disabled={!artifactTitle.trim() || !artifactContent.trim() || busy} />
              {artifacts.map(artifact => <View key={artifact.id} style={styles.listRow}><Text style={styles.rowTitle}>{artifact.title}</Text><Text style={styles.rowMeta} numberOfLines={2}>{artifact.content}</Text></View>)}
              {artifacts.length === 0 && <Text style={styles.muted}>No captured context yet.</Text>}
            </View>

            <View style={styles.panel}>
              <Text style={styles.panelTitle}>03 · Create handoff</Text>
              <TextInput value={taskTitle} onChangeText={setTaskTitle} placeholder="Task title" style={styles.input} />
              <TextInput value={taskObjective} onChangeText={setTaskObjective} placeholder="Expected outcome and instructions…" style={[styles.input, styles.multiline]} multiline textAlignVertical="top" />
              <Text style={styles.fieldLabel}>ASSIGN TO</Text>
              {agents.map(agent => (
                <Pressable key={agent.id} onPress={() => setAssignee(agent.id)} style={[styles.agentChoice, assignee === agent.id && styles.agentChoiceActive]}>
                  <Text style={styles.rowTitle}>{agent.display_name}</Text><Text style={styles.rowMeta}>{agent.provider}</Text>
                </Pressable>
              ))}
              <Text style={styles.fieldLabel}>ATTACH CONTEXT</Text>
              {artifacts.map(artifact => (
                <Pressable key={artifact.id} onPress={() => setSelectedArtifactIds(current => current.includes(artifact.id) ? current.filter(id => id !== artifact.id) : [...current, artifact.id])} style={styles.contextChoice}>
                  <Text style={styles.rowTitle}>{selectedArtifactIds.includes(artifact.id) ? "☑" : "□"}  {artifact.title}</Text>
                </Pressable>
              ))}
              <ActionButton label="Create handoff" onPress={createTask} disabled={!taskTitle.trim() || !taskObjective.trim() || !assignee || busy} />
              {tasks.map(task => (
                <View key={task.id} style={styles.listRow}>
                  <Text style={styles.rowTitle}>{task.title}</Text>
                  <Text style={styles.rowMeta}>{task.status.replaceAll("_", " ")} · version {task.version}</Text>
                  <Text style={styles.rowMeta}>{task.objective}</Text>
                  {task.status === "in_progress" ? (
                    <>
                      <TextInput value={responseDrafts[task.id] ?? ""} onChangeText={value => setResponseDrafts(current => ({ ...current, [task.id]: value }))} placeholder="Paste the assistant response…" style={[styles.input, styles.multiline]} multiline textAlignVertical="top" />
                      <ActionButton label="Submit response" onPress={() => void submitTaskResponse(task)} disabled={!(responseDrafts[task.id] ?? "").trim() || busy} />
                    </>
                  ) : null}
                  {task.status === "review_required" ? <ActionButton label="Request changes" onPress={() => void advanceTask(task, "changes_requested")} disabled={busy} secondary /> : null}
                  {nextTaskStatus[task.status] ? <ActionButton label={`Advance to ${nextTaskStatus[task.status].replaceAll("_", " ")}`} onPress={() => void advanceTask(task, nextTaskStatus[task.status])} disabled={busy} secondary /> : null}
                  {task.response_artifact_id ? <Text style={styles.responseNote}>Response artifact linked · {task.response_artifact_id.slice(0, 8)}</Text> : null}
                </View>
              ))}
              {tasks.length === 0 && <Text style={styles.muted}>No handoffs yet.</Text>}
            </View>
          </> : <>
            <TextInput value={workspaceName} onChangeText={setWorkspaceName} placeholder="New workspace name" placeholderTextColor="#718073" style={styles.input}/>
            <ActionButton label="Create workspace" onPress={createWorkspace} disabled={!workspaceName.trim()||busy}/>
            {workspaces.map(workspace=><Pressable key={workspace.id} onPress={()=>setSelected(workspace)} style={styles.workspaceRow}><View style={styles.workspaceGlyph}><Text style={styles.glyphText}>{workspace.name.slice(0,1).toUpperCase()}</Text></View><View style={styles.workspaceCopy}><Text style={styles.rowTitle}>{workspace.name}</Text><Text style={styles.rowMeta}>Open workspace</Text></View><Text style={styles.chevron}>›</Text></Pressable>)}
          </>}
        </ScrollView>}
        {error ? <View style={{position:"absolute",left:12,right:12,bottom:86,padding:11,borderRadius:11,backgroundColor:"#43292d"}}><Text style={{color:"#ffb8b8",fontSize:10}}>{error}</Text></View> : null}
      </View>

      <View style={{position:"absolute",bottom:13,alignSelf:"center",flexDirection:"row",gap:4,padding:5,borderRadius:30,backgroundColor:"#19251b",borderWidth:1,borderColor:"#344336",elevation:12}}>
        <Pressable onPress={()=>setActiveTab("chats")} style={{minWidth:112,flexDirection:"row",alignItems:"center",justifyContent:"center",gap:8,paddingHorizontal:17,paddingVertical:11,borderRadius:24,backgroundColor:activeTab==="chats"?"#bfff4b":"transparent"}}><Text style={{color:activeTab==="chats"?"#17200f":"#a4b0a2",fontSize:11,fontWeight:"800"}}>◉  Chats</Text></Pressable>
        <Pressable onPress={()=>setActiveTab("explore")} style={{minWidth:112,flexDirection:"row",alignItems:"center",justifyContent:"center",gap:8,paddingHorizontal:17,paddingVertical:11,borderRadius:24,backgroundColor:activeTab==="explore"?"#bfff4b":"transparent"}}><Text style={{color:activeTab==="explore"?"#17200f":"#a4b0a2",fontSize:11,fontWeight:"800"}}>⌕  Explore</Text></Pressable>
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
  warning: { color: axonTheme.color.warning, fontSize: 10, lineHeight: 15, marginTop: 10, textAlign: "center" }
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
  if (user) return <WorkspaceApp/>;
  return <SafeAreaView style={styles.safe}><ScrollView contentContainerStyle={styles.page} keyboardShouldPersistTaps="handled"><View style={styles.brandRow}><View style={styles.logo}><Text style={styles.logoText}>A</Text></View><Text style={styles.brand}>AXON</Text></View><Text style={styles.eyebrow}>SECURE ACCESS</Text><Text style={styles.title}>{createAccount ? "Create your account" : "Welcome back"}</Text><Text style={styles.description}>Sign in to access your personal workspace and its data.</Text><TextInput value={email} onChangeText={setEmail} placeholder="Email address" keyboardType="email-address" autoCapitalize="none" autoComplete="email" style={styles.input}/><TextInput value={password} onChangeText={setPassword} placeholder="Password" secureTextEntry autoCapitalize="none" autoComplete={createAccount ? "new-password" : "password"} style={styles.input}/><ActionButton label={working ? "Please wait…" : createAccount ? "Create account" : "Sign in"} onPress={()=>void submit()} disabled={working || !email.trim() || password.length<6}/>{authError ? <Text style={styles.errorText}>{authError}</Text> : null}<Text onPress={()=>setCreateAccount(value=>!value)} style={styles.back}>{createAccount ? "Already registered? Sign in" : "New to Axon? Create an account"}</Text></ScrollView></SafeAreaView>;
}

import { onAuthStateChanged, signInWithEmailAndPassword, createUserWithEmailAndPassword, signOut } from "firebase/auth";

export default function App() { return <AuthGate />; }
