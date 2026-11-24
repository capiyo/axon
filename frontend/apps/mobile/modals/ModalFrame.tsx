import type { ReactNode } from "react";
import { View, type StyleProp, type ViewStyle } from "react-native";

interface ModalFrameProps { children: ReactNode; style?: StyleProp<ViewStyle>; }

export default function ModalFrame({ children, style }: ModalFrameProps) {
  return <View style={style}>{children}</View>;
}
