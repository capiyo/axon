import type { ReactNode } from "react";
import { Modal, SafeAreaView, ScrollView, StyleSheet, View } from "react-native";
import { axonTheme } from "../../../packages/theme/src";

interface ProfileModalProps {
  visible: boolean;
  onClose: () => void;
  children: ReactNode;
}

export default function ProfileModal({ visible, onClose, children }: ProfileModalProps) {
  return (
    <Modal visible={visible} transparent animationType="slide" statusBarTranslucent onRequestClose={onClose}>
      <View style={styles.backdrop}>
        <SafeAreaView style={styles.safeArea}>
          <View style={styles.sheet}>
            <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
              {children}
            </ScrollView>
          </View>
        </SafeAreaView>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: { flex: 1, justifyContent: "flex-end", backgroundColor: "rgba(0,0,0,0.52)" },
  safeArea: { flex: 1, justifyContent: "flex-end" },
  sheet: { maxHeight: "94%", minHeight: "72%", overflow: "hidden", borderTopLeftRadius: 24, borderTopRightRadius: 24, backgroundColor: axonTheme.color.canvas },
  content: { paddingHorizontal: 16, paddingTop: 14, paddingBottom: 30 }
});
