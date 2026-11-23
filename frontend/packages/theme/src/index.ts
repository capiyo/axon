/** Axon's independent deep-indigo and cyan design system. */
export const axonTheme = {
  color: {
    canvas: "#0b1020",
    canvasGlow: "#17264a",
    surface: "#111a2e",
    surfaceRaised: "#1b2942",
    surfaceInput: "#0b1326",
    border: "#263654",
    borderStrong: "#3a5278",
    text: "#edf3ff",
    textMuted: "#a0b2d0",
    textSubtle: "#788cac",
    accent: "#67d8f0",
    accentText: "#071a24",
    success: "#63d7bd",
    warning: "#ffd58a",
    danger: "#ff999f",
  },
  radius: { small: "8px", medium: "12px", large: "17px", panel: "14px" },
  spacing: { xs: "4px", sm: "8px", md: "12px", lg: "16px", xl: "24px" },
} as const;

export type AxonTheme = typeof axonTheme;
