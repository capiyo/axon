type HomeTab = "chats" | "explore";
interface HomeScreenProps { activeTab: HomeTab; unreadCount: number; onTabChange: (tab: HomeTab) => void; }
export default function HomeScreen({ activeTab, unreadCount, onTabChange }: HomeScreenProps) {
  return <nav className="axon-home-tabs" aria-label="Home tabs">
    <button type="button" className={activeTab === "chats" ? "active" : ""} aria-current={activeTab === "chats" ? "page" : undefined} onClick={() => onTabChange("chats")}>Chats{unreadCount > 0 && <span className="axon-home-unread">{unreadCount > 99 ? "99+" : unreadCount}</span>}</button>
    <button type="button" className={activeTab === "explore" ? "active" : ""} aria-current={activeTab === "explore" ? "page" : undefined} onClick={() => onTabChange("explore")}>Explore</button>
  </nav>;
}
