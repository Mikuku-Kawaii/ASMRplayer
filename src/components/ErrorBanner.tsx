import React, { useState } from "react";
import {
  AlertTriangle,
  Copy,
  Check,
  RefreshCw,
  RotateCcw,
  X,
} from "lucide-react";
import { AppError } from "../lib/api";

interface ErrorBannerProps {
  error: AppError | null;
  onRetry?: () => void;
  onRestartEngine?: () => void;
  onDismiss?: () => void;
}

export const ErrorBanner: React.FC<ErrorBannerProps> = ({
  error,
  onRetry,
  onRestartEngine,
  onDismiss,
}) => {
  const [copied, setCopied] = useState(false);

  if (!error) return null;

  const handleCopy = () => {
    const text = `[${error.code}] ${error.message}${
      error.hint ? ` (${error.hint})` : ""
    }`;
    void navigator.clipboard?.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1800);
  };

  return (
    <aside
      role="alert"
      className="error-banner error-popup-topleft"
      aria-live="assertive"
      aria-label="错误提示弹窗"
    >
      <div className="error-popup-header">
        <div className="error-popup-title-group">
          <AlertTriangle size={15} className="error-icon" />
          <span className="error-popup-title">错误提示</span>
          <span className="error-code-badge">{error.code}</span>
        </div>
        {onDismiss && (
          <button
            type="button"
            className="error-popup-close"
            onClick={onDismiss}
            aria-label="关闭错误提示"
            title="关闭错误提示"
          >
            <X size={14} />
          </button>
        )}
      </div>

      <div className="error-popup-body">
        <div className="error-message">{error.message}</div>
        {error.hint && <p className="error-hint">{error.hint}</p>}
      </div>

      <div className="error-actions">
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          onClick={handleCopy}
          title="复制诊断错误码"
        >
          {copied ? <Check size={13} /> : <Copy size={13} />}
          <span>{copied ? "已复制" : "复制诊断码"}</span>
        </button>
        {onRestartEngine && (
          <button
            type="button"
            className="btn btn-secondary btn-sm"
            onClick={onRestartEngine}
            aria-label="重启播放引擎"
            title="重启 mpv 播放引擎并保留当前位置"
          >
            <RotateCcw size={13} />
            <span>重启引擎</span>
          </button>
        )}
        {onRetry && (
          <button
            type="button"
            className="btn btn-secondary btn-sm"
            onClick={onRetry}
          >
            <RefreshCw size={13} />
            <span>重试</span>
          </button>
        )}
      </div>
    </aside>
  );
};
