import type { Notification } from "../../../../packages/contracts/src";
import { Pressable, ScrollView, Text, View } from "react-native";
import ModalFrame from "./ModalFrame";

interface NotificationsModalProps {
  notifications: Notification[];
  style: object;
  onMarkAllRead: () => void;
  onClose: () => void;
  onOpenNotification: (notification: Notification) => void;
  styles: Record<string, object>;
}

export default function NotificationsModal({ notifications, style, onMarkAllRead, onClose, onOpenNotification, styles }: NotificationsModalProps) {
  return (
    <ModalFrame style={style}>
      <View style={styles.notificationsHeader}><Text style={styles.screenTitle}>Notifications</Text><View style={styles.notificationsHeaderActions}><Pressable onPress={onMarkAllRead}><Text style={styles.exploreActionText}>Mark all read</Text></Pressable><Pressable onPress={onClose}><Text style={styles.exploreActionText}>Close</Text></Pressable></View></View>
      <ScrollView contentContainerStyle={styles.notificationsList}>
        {notifications.map(item => <Pressable key={item.id} onPress={() => onOpenNotification(item)} style={[styles.notificationItemMobile,Boolean(item.read_at) && styles.notificationReadMobile]}><Text style={styles.rowTitle}>{item.title}</Text><Text style={styles.rowMeta}>{item.body}</Text><Text style={styles.notificationTime}>{new Date(item.created_at).toLocaleString()}</Text></Pressable>)}
        {notifications.length===0 ? <Text style={styles.muted}>You are all caught up.</Text> : null}
      </ScrollView>
    </ModalFrame>
  );
}
