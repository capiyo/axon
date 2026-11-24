import type { Chat } from "../../../packages/contracts/src";
import { Pressable, Text, View, type TextStyle, type ViewStyle } from "react-native";

interface ChatCardStyles {
  chatCardMobile: ViewStyle;
  chatCardMobileHeader: ViewStyle;
  chatCardMobileAvatar: ViewStyle;
  chatCardMobileAvatarText: TextStyle;
  chatCardMobileTitle: TextStyle;
  chatCardMobileAgent: TextStyle;
  chatCardMobileTime: TextStyle;
  chatCardMobilePreview: TextStyle;
  chatCardMobileProgress: ViewStyle;
  chatCardMobileProgressFill: ViewStyle;
  chatCardMobileOpen: TextStyle;
  chatCardMobileFooter: ViewStyle;
  chatCardMobileMeta: TextStyle;
  chatCardMobileActions: ViewStyle;
  chatUnreadCount: TextStyle;
  chatCardMobileLike: TextStyle;
  chatCardMobileArchive: TextStyle;
}

interface ChatCardProps {
  chat: Chat;
  linkedTaskCount: number;
  progress: number | null;
  styles: ChatCardStyles;
  onOpen: () => void;
  onLike: () => void;
  onArchive: () => void;
}

export default function ChatCard({ chat, linkedTaskCount, progress, styles, onOpen, onLike, onArchive }: ChatCardProps) {
  const last = chat.last_message;
  const replyLabel = last?.role === "assistant" ? "Agent replied" : last?.role === "user" ? "Your message" : "New conversation";
  return (
    <View style={styles.chatCardMobile}>
      <Pressable onPress={onOpen}>
        <View style={styles.chatCardMobileHeader}>
          <View style={styles.chatCardMobileAvatar}><Text style={styles.chatCardMobileAvatarText}>{(last?.answered_by || chat.title || "A").slice(0, 1).toUpperCase()}</Text></View>
          <View style={{ flex: 1 }}><Text style={styles.chatCardMobileTitle}>{chat.title}</Text><Text style={styles.chatCardMobileAgent}>{last?.answered_by || "Workspace conversation"}</Text></View>
          <Text style={styles.chatCardMobileTime}>{new Date(chat.updated_at).toLocaleString()}</Text>
        </View>
        <Text style={styles.chatCardMobilePreview}>{last?.content || "No messages yet. Open this conversation to begin."}</Text>
        {progress !== null ? <View style={styles.chatCardMobileProgress}><View style={[styles.chatCardMobileProgressFill, { width: `${progress}%` }]} /></View> : null}
        <Text style={styles.chatCardMobileOpen}>OPEN CONVERSATION →</Text>
      </Pressable>
      <View style={styles.chatCardMobileFooter}>
        <Text style={styles.chatCardMobileMeta}>{chat.message_count} messages · {linkedTaskCount} tasks · {replyLabel}</Text>
        <View style={styles.chatCardMobileActions}>
          {chat.unread_count > 0 ? <Text style={styles.chatUnreadCount}>{chat.unread_count}</Text> : null}
          <Pressable onPress={onLike}><Text style={styles.chatCardMobileLike}>{chat.is_liked ? "♥" : "♡"}</Text></Pressable>
          <Pressable onPress={onArchive}><Text style={styles.chatCardMobileArchive}>Archive</Text></Pressable>
        </View>
      </View>
    </View>
  );
}
