import { ReactNode, useState } from "react";
import { Toast } from "./Toast";

/** Presentation only: dismissing a notice never clears the underlying failure state. */
export function ErrorNotice({ message, children, tone = "error" }: {
  message?: string | null; children?: ReactNode; tone?: "error" | "warning";
}) {
  return message ? <Notice key={message} message={message} tone={tone}>{children}</Notice> : null;
}

function Notice({ message, children, tone }: { message: string; children?: ReactNode; tone: "error" | "warning" }) {
  const [dismissed, setDismissed] = useState(false);
  return dismissed ? null : <Toast message={message} tone={tone} duration={children ? 0 : 3000}
    onClose={() => setDismissed(true)}>{children}</Toast>;
}
