import type { ReactNode } from "react";
import { View } from "react-native";
interface ChatsScreenProps { children: ReactNode; }
export default function ChatsScreen({ children }: ChatsScreenProps) {
  return <View accessibilityLabel="Chats screen">{children}</View>;
}
