import type { ReactNode } from "react";
import { SafeAreaView, type StyleProp, type ViewStyle } from "react-native";

interface WorkspaceViewProps { children: ReactNode; style?: StyleProp<ViewStyle>; }

export default function WorkspaceView({ children, style }: WorkspaceViewProps) {
  return <SafeAreaView style={style}>{children}</SafeAreaView>;
}
