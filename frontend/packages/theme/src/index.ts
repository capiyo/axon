/** Axon's Neural Spectrum design system, shared by web and native clients. */
export const axonTheme = {
  color: {
    canvas: "#080D1F",
    canvasGlow: "#0D1733",
    surface: "#101B38",
    surfaceRaised: "#152449",
    surfaceInput: "#0B1530",
    border: "#263B70",
    borderStrong: "#2868C8",
    text: "#EAFBFF",
    textMuted: "#9DB8D5",
    textSubtle: "#7188B3",
    accent: "#25D9E8",
    accentText: "#061426",
    cyan: "#25D9E8",
    blue: "#1879F5",
    violet: "#8545F5",
    success: "#31D6C5",
    warning: "#F1C66D",
    danger: "#F08CA5",
    floating: "#14244A",
    selected: "#173C77",
    primary: "#126BE8",
  },
  radius: { small: "8px", medium: "12px", large: "17px", panel: "14px", card: "18px" },
  spacing: { xs: "4px", sm: "8px", md: "12px", lg: "16px", xl: "24px" },
} as const;

export type AxonTheme = typeof axonTheme;
