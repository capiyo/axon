import type { Chat } from "../../../packages/contracts/src";
import { Pressable, Text, View, type TextStyle, type ViewStyle } from "react-native";

interface SessionCardStyles {
  sessionCardMobile: ViewStyle;
  sessionCardMobileHeader: ViewStyle;
  sessionCardMobileAvatar: ViewStyle;
  sessionCardMobileAvatarText: TextStyle;
  sessionCardMobileTitle: TextStyle;
  sessionCardMobileAgent: TextStyle;
  sessionCardMobileTime: TextStyle;
  sessionCardMobilePreview: TextStyle;
  sessionCardMobileProgress: ViewStyle;
  sessionCardMobileProgressFill: ViewStyle;
  sessionCardMobileOpen: TextStyle;
  sessionCardMobileFooter: ViewStyle;
  sessionCardMobileMeta: TextStyle;
  sessionCardMobileActions: ViewStyle;
  sessionUnreadCount: TextStyle;
  sessionCardMobileLike: TextStyle;
  sessionCardMobileArchive: TextStyle;
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
  const replyLabel = last?.role === "assistant" ? "Agent replied" : last?.role === "user" ? "Your message" : "New session";
  return (
    <View style={styles.sessionCardMobile}>
      <Pressable onPress={onOpen}>
        <View style={styles.sessionCardMobileHeader}>
          <View style={styles.sessionCardMobileAvatar}><Text style={styles.sessionCardMobileAvatarText}>{(last?.answered_by || session.title || "A").slice(0, 1).toUpperCase()}</Text></View>
          <View style={{ flex: 1 }}><Text style={styles.sessionCardMobileTitle}>{session.title}</Text><Text style={styles.sessionCardMobileAgent}>{last?.answered_by || "Session"}</Text></View>
          <Text style={styles.sessionCardMobileTime}>{new Date(session.updated_at).toLocaleString()}</Text>
        </View>
        <Text style={styles.sessionCardMobilePreview}>{last?.content || "No messages yet. Open this session to begin."}</Text>
        {progress !== null ? <View style={styles.sessionCardMobileProgress}><View style={[styles.sessionCardMobileProgressFill, { width: `${progress}%` }]} /></View> : null}
        <Text style={styles.sessionCardMobileOpen}>OPEN SESSION →</Text>
      </Pressable>
      <View style={styles.sessionCardMobileFooter}>
        <Text style={styles.sessionCardMobileMeta}>{session.message_count} messages · {linkedTaskCount} tasks · {replyLabel}</Text>
        <View style={styles.sessionCardMobileActions}>
          {session.unread_count > 0 ? <Text style={styles.sessionUnreadCount}>{session.unread_count}</Text> : null}
          <Pressable onPress={onLike}><Text style={styles.sessionCardMobileLike}>{session.is_liked ? "♥" : "♡"}</Text></Pressable>
          <Pressable onPress={onArchive}><Text style={styles.sessionCardMobileArchive}>Archive</Text></Pressable>
        </View>
      </View>
    </View>
  );
}
