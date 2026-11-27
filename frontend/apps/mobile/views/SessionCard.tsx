import type { Chat } from "../../../packages/contracts/src";
import { Pressable, Text, View, type TextStyle, type ViewStyle } from "react-native";

interface SessionCardStyles {
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

interface SessionCardProps {
  session: Chat;
  linkedTaskCount: number;
  progress: number | null;
  styles: SessionCardStyles;
  onOpen: () => void;
  onLike: () => void;
  onArchive: () => void;
}

export default function SessionCard({ session, linkedTaskCount, progress, styles, onOpen, onLike, onArchive }: SessionCardProps) {
  const last = session.last_message;
  const replyLabel = last?.role === "assistant" ? "Axon responded" : last?.role === "user" ? "Your prompt" : "New session";
  return (
    <View style={styles.chatCardMobile}>
      <Pressable onPress={onOpen}>
        <View style={styles.chatCardMobileHeader}>
          <View style={styles.chatCardMobileAvatar}><Text style={styles.chatCardMobileAvatarText}>{(last?.answered_by || session.title || "A").slice(0, 1).toUpperCase()}</Text></View>
          <View style={{ flex: 1 }}><Text style={styles.chatCardMobileTitle}>{session.title}</Text><Text style={styles.chatCardMobileAgent}>{last?.answered_by || "Session"}</Text></View>
          <Text style={styles.chatCardMobileTime}>{new Date(session.updated_at).toLocaleString()}</Text>
        </View>
        <Text style={styles.chatCardMobilePreview}>{last?.content || "No messages yet. Open this session to begin."}</Text>
        {progress !== null ? <View style={styles.chatCardMobileProgress}><View style={[styles.chatCardMobileProgressFill, { width: `${progress}%` }]} /></View> : null}
        <Text style={styles.chatCardMobileOpen}>OPEN SESSION →</Text>
      </Pressable>
      <View style={styles.chatCardMobileFooter}>
        <Text style={styles.chatCardMobileMeta}>{session.message_count} turns · {linkedTaskCount} tasks · {replyLabel}</Text>
        <View style={styles.chatCardMobileActions}>
          {session.unread_count > 0 ? <Text style={styles.chatUnreadCount}>{session.unread_count}</Text> : null}
          <Pressable onPress={onLike}><Text style={styles.chatCardMobileLike}>{session.is_liked ? "♥" : "♡"}</Text></Pressable>
          <Pressable onPress={onArchive}><Text style={styles.chatCardMobileArchive}>Archive</Text></Pressable>
        </View>
      </View>
    </View>
  );
}
