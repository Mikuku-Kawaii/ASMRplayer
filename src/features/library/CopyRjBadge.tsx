import { useEffect, useRef, useState } from "react";

export function CopyRjBadge({ code }: { code: string }) {
  const [feedback, setFeedback] = useState("");
  const [copied, setCopied] = useState(false);
  const attempt = useRef(0);
  const resetTimer = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => () => {
    attempt.current += 1;
    clearTimeout(resetTimer.current);
  }, [code]);

  const copy = async () => {
    const current = ++attempt.current;
    clearTimeout(resetTimer.current);
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
    resetTimer.current = setTimeout(() => {
      setCopied(false);
      setFeedback("");
    }, 1800);
  };

  return (
    <button
      type="button"
      className={`badge badge-rj badge-rj-copy${copied ? " is-copied" : ""}`}
      onClick={() => void copy()}
      aria-label={`复制编号 ${code}`}
      title={feedback || `复制编号 ${code}`}
    >
      {code}
      <span className="badge-copy-feedback" role="status">{feedback}</span>
    </button>
  );
}
