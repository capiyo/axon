import { Pressable, Text, View } from "react-native";
import { axonTheme } from "../../../packages/theme/src";
type HomeTab = "chats" | "explore";
interface HomeScreenProps { activeTab: HomeTab; unreadCount: number; onTabChange: (tab: HomeTab) => void; }
export default function HomeScreen({ activeTab, unreadCount, onTabChange }: HomeScreenProps) {
  return <View accessibilityLabel="Home tabs" style={{ flexDirection: "row", gap: 8, paddingVertical: 8, marginBottom: 12 }}>
    <Pressable accessibilityRole="tab" accessibilityState={{ selected: activeTab === "chats" }} onPress={() => onTabChange("chats")} style={{ flex: 1, minHeight: 42, borderRadius: 22, backgroundColor: activeTab === "chats" ? "#244B33" : "#14231B", alignItems: "center", justifyContent: "center", flexDirection: "row", gap: 7 }}>
      <Text style={{ color: activeTab === "chats" ? "#E7F6EB" : axonTheme.color.textMuted, fontSize: 13, fontWeight: "800" }}>Chats</Text>
      {unreadCount > 0 ? <Text style={{ color: "#E7F6EB", backgroundColor: "#28583D", overflow: "hidden", borderRadius: 10, paddingHorizontal: 6, fontSize: 10, fontWeight: "900" }}>{unreadCount > 99 ? "99+" : unreadCount}</Text> : null}
    </Pressable>
    <Pressable accessibilityRole="tab" accessibilityState={{ selected: activeTab === "explore" }} onPress={() => onTabChange("explore")} style={{ flex: 1, minHeight: 42, borderRadius: 22, backgroundColor: activeTab === "explore" ? "#244B33" : "#14231B", alignItems: "center", justifyContent: "center" }}>
      <Text style={{ color: activeTab === "explore" ? "#E7F6EB" : axonTheme.color.textMuted, fontSize: 13, fontWeight: "800" }}>Explore</Text>
    </Pressable>
  </View>;
}
