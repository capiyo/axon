import { Pressable, Text, View } from "react-native";
import { axonTheme } from "../../../packages/theme/src";
type HomeTab = "chats" | "explore";
interface HomeScreenProps { activeTab: HomeTab; unreadCount: number; onTabChange: (tab: HomeTab) => void; }
export default function HomeScreen({ activeTab, unreadCount, onTabChange }: HomeScreenProps) {
  return <View accessibilityLabel="Hub navigation" style={{ position: "absolute", zIndex: 25, bottom: 20, alignSelf: "center", width: 218, flexDirection: "row", gap: 5, padding: 5, borderRadius: 999, backgroundColor: "#1C3024", elevation: 14, shadowColor: "#000", shadowOpacity: 0.24, shadowRadius: 12, shadowOffset: { width: 0, height: 5 } }}>
    <Pressable accessibilityRole="tab" accessibilityState={{ selected: activeTab === "chats" }} onPress={() => onTabChange("chats")} style={{ flex: 1, minHeight: 42, borderRadius: 22, backgroundColor: activeTab === "chats" ? "#244B33" : "#14231B", alignItems: "center", justifyContent: "center", flexDirection: "row", gap: 7 }}>
      <Text style={{ color: activeTab === "chats" ? "#E7F6EB" : axonTheme.color.textMuted, fontSize: 13, fontWeight: "800" }}>Sessions</Text>
      {unreadCount > 0 ? <Text style={{ color: "#E7F6EB", backgroundColor: "#28583D", overflow: "hidden", borderRadius: 10, paddingHorizontal: 6, fontSize: 10, fontWeight: "900" }}>{unreadCount > 99 ? "99+" : unreadCount}</Text> : null}
    </Pressable>
    <Pressable accessibilityRole="tab" accessibilityState={{ selected: activeTab === "explore" }} onPress={() => onTabChange("explore")} style={{ flex: 1, minHeight: 42, borderRadius: 22, backgroundColor: activeTab === "explore" ? "#244B33" : "#14231B", alignItems: "center", justifyContent: "center" }}>
      <Text style={{ color: activeTab === "explore" ? "#E7F6EB" : axonTheme.color.textMuted, fontSize: 13, fontWeight: "800" }}>Explore</Text>
    </Pressable>
  </View>;
}
