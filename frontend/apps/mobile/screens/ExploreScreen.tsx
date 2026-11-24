import type { ReactNode } from "react";
import { View } from "react-native";
interface ExploreScreenProps { children: ReactNode; }
export default function ExploreScreen({ children }: ExploreScreenProps) {
  return <View accessibilityLabel="Explore screen">{children}</View>;
}
