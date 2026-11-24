import type { ReactNode } from "react";

interface ModalFrameProps {
  children: ReactNode;
  className: string;
  role?: string;
  ariaModal?: "true" | "false";
  ariaLabel?: string;
}

export default function ModalFrame({ children, className, role, ariaModal, ariaLabel }: ModalFrameProps) {
  return <div className={className} role={role} aria-modal={ariaModal} aria-label={ariaLabel}>{children}</div>;
}
