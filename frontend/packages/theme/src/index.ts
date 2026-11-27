/** Axon's Forest & Mint design system, shared by web and native clients. */
export const axonTheme = {
  color: {
    canvas: "#0E1612",
    canvasGlow: "#14231B",
    surface: "#14231B",
    surfaceRaised: "#17271D",
    surfaceInput: "#101D15",
    border: "#294034",
    borderStrong: "#386E4B",
    text: "#E7F6EB",
    textMuted: "#9AAFA1",
    textSubtle: "#82998A",
    accent: "#9DE4B0",
    accentText: "#0E1612",
    success: "#9DE4B0",
    warning: "#E9C779",
    danger: "#F29A9A",
    floating: "#1C3024",
    selected: "#244B33",
    primary: "#28583D",
  },
  radius: { small: "8px", medium: "12px", large: "17px", panel: "14px", card: "18px" },
  spacing: { xs: "4px", sm: "8px", md: "12px", lg: "16px", xl: "24px" },
} as const;

export type AxonTheme = typeof axonTheme;
