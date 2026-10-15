import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, AlertCircle, X } from "lucide-react";

export function CopyRjBadge({ code }: { code: string }) {
  const [feedback, setFeedback] = useState("");
  const [copied, setCopied] = useState(false);
  const [closing, setClosing] = useState(false);
  const attempt = useRef(0);
  const resetTimer = useRef<ReturnType<typeof setTimeout>>();
  const exitTimer = useRef<ReturnType<typeof setTimeout>>();
  const badgeRef = useRef<HTMLButtonElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => () => {
    attempt.current += 1;
    clearTimeout(resetTimer.current);
    clearTimeout(exitTimer.current);
  }, [code]);

  const dismiss = () => {
    clearTimeout(resetTimer.current);
    clearTimeout(exitTimer.current);
    if (document.activeElement === closeRef.current) badgeRef.current?.focus();
    setClosing(true);
    const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    exitTimer.current = setTimeout(() => {
      setCopied(false);
      setFeedback("");
      setClosing(false);
    }, reducedMotion ? 0 : 180);
  };

  const copy = async () => {
    const current = ++attempt.current;
    clearTimeout(resetTimer.current);
    clearTimeout(exitTimer.current);
    setClosing(false);
    setCopied(false);
    setFeedback("");
    try {
      await navigator.clipboard.writeText(code);
      if (current !== attempt.current) return;
      setCopied(true);
      setFeedback(`已复制 ${code}`);
    } catch {
      if (current !== attempt.current) return;
      setFeedback("复制失败，请重试");
    }
    resetTimer.current = setTimeout(dismiss, 2000);
  };

  return (
    <><button
      ref={badgeRef}
      type="button"
      className={`badge badge-rj badge-rj-copy${copied ? " is-copied" : ""}`}
      onClick={() => void copy()}
      aria-label={`复制编号 ${code}`}
      title={feedback || `复制编号 ${code}`}
    >
      {code}
    </button>
    {feedback && createPortal(
      <div className={`copy-feedback-toast${copied ? "" : " is-error"}${closing ? " is-closing" : ""}`}>
        {copied ? <Check size={18} aria-hidden="true" /> : <AlertCircle size={18} aria-hidden="true" />}
        <span className="ui-icon-label" role="status" aria-live="polite" aria-atomic="true">{copied ? `复制成功 · ${code}` : feedback}</span>
        <button ref={closeRef} type="button" className="btn btn-icon copy-feedback-close" aria-label="关闭复制提示" title="关闭复制提示" disabled={closing} onClick={dismiss}>
          <X size={16} aria-hidden="true" />
        </button>
      </div>, document.body,
    )}</>
  );
}
