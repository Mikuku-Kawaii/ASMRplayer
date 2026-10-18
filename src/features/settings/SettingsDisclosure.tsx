import { useId, useState, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";

/** Keep editors mounted so folding a group never discards an unfinished draft. */
export function SettingsDisclosure({ title, description, summary, defaultOpen = false, expanded: controlledExpanded, onExpandedChange, children }: {
  title: string;
  description?: string;
  summary?: ReactNode;
  defaultOpen?: boolean;
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  children: ReactNode;
}) {
  const id = useId();
  const [localExpanded, setLocalExpanded] = useState(defaultOpen);
  const expanded = controlledExpanded ?? localExpanded;
  const toggle = () => {
    if (controlledExpanded === undefined) setLocalExpanded(!expanded);
    onExpandedChange?.(!expanded);
  };
  return <section className="settings-disclosure" aria-labelledby={`${id}-title`}>
    <button type="button" className="settings-disclosure-trigger" aria-expanded={expanded} aria-controls={`${id}-content`} onClick={toggle}>
      <span className="settings-label-block"><span id={`${id}-title`} className="settings-disclosure-title">{title}</span>{description && <span className="settings-field-hint">{description}</span>}</span>
      <span className="settings-disclosure-state">{summary && <span>{summary}</span>}<ChevronDown size={18} aria-hidden="true" /></span>
    </button>
    <div id={`${id}-content`} className="settings-disclosure-content" hidden={!expanded}>{children}</div>
  </section>;
}
