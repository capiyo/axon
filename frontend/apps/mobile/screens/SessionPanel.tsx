import type { ReactNode } from "react";
import { View } from "react-native";
interface SessionPanelProps { children: ReactNode; }
export default function SessionPanel({ children }: SessionPanelProps) {
  return <View accessibilityLabel="Session workspace">{children}</View>;
}
