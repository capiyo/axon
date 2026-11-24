import { Pressable, Text, View } from "react-native";
import { axonTheme } from "../../../packages/theme/src";
interface HomeScreenProps { agentCount: number; chatCount: number; runningCount: number; completedCount: number; artifactCount: number; onOpenChats: () => void; onOpenExplore: () => void; }
export default function HomeScreen({ agentCount, chatCount, runningCount, completedCount, artifactCount, onOpenChats, onOpenExplore }: HomeScreenProps) {
  const metrics = [{ label: "Agents", value: agentCount }, { label: "Chats", value: chatCount }, { label: "Running", value: runningCount }, { label: "Completed", value: completedCount }, { label: "Artifacts", value: artifactCount }];
  return <View style={{ gap: 16 }}>
    <View style={{ paddingVertical: 12 }}><Text style={{ color: axonTheme.color.accent, fontSize: 10, fontWeight: "800", letterSpacing: 1.6 }}>AXON HUB</Text><Text style={{ color: axonTheme.color.text, fontSize: 28, fontWeight: "800", marginTop: 8 }}>Your command center</Text><Text style={{ color: axonTheme.color.textMuted, fontSize: 14, lineHeight: 22, marginTop: 8 }}>See what your agents are doing, continue conversations, and discover capabilities for your work.</Text></View>
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 10 }}>{metrics.map(item => <View key={item.label} style={{ width: "47%", minHeight: 84, padding: 14, borderRadius: 16, backgroundColor: axonTheme.color.surface, justifyContent: "center" }}><Text style={{ color: axonTheme.color.text, fontSize: 25, fontWeight: "800" }}>{item.value}</Text><Text style={{ color: axonTheme.color.textMuted, fontSize: 12, marginTop: 4 }}>{item.label}</Text></View>)}</View>
    <Pressable onPress={onOpenChats} style={{ padding: 17, borderRadius: 16, backgroundColor: axonTheme.color.surface, gap: 5 }}><Text style={{ color: axonTheme.color.text, fontSize: 16, fontWeight: "800" }}>Open Chats</Text><Text style={{ color: axonTheme.color.textMuted, fontSize: 13 }}>Continue work and shared context</Text></Pressable>
    <Pressable onPress={onOpenExplore} style={{ padding: 17, borderRadius: 16, backgroundColor: axonTheme.color.surface, gap: 5 }}><Text style={{ color: axonTheme.color.text, fontSize: 16, fontWeight: "800" }}>Explore agents</Text><Text style={{ color: axonTheme.color.textMuted, fontSize: 13 }}>Discover and connect new capabilities</Text></Pressable>
  </View>;
}
