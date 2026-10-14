import React from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { AppError, callCommand, normalizeAppError } from "../../lib/api";

export const NOTES_LIMIT = 50_000;
// T1409: absolute web URLs only. Never pass raw release content to a shell or WebView.
export function safeWebUrl(value: string): string | undefined {
  if (!/^https?:\/\//i.test(value) || /[\s\\]/.test(value) || [...value].some(c => c.charCodeAt(0) < 32 || c.charCodeAt(0) === 127)) return;
  try {
    const url = new URL(value);
    if (url.hostname && !url.username && !url.password) return url.href;
  } catch { /* Untrusted malformed URL becomes plain text. */ }
}

class NotesBoundary extends React.Component<{ text: string; children: React.ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() { return this.state.failed ? <pre className="release-notes-fallback">{this.props.text}</pre> : this.props.children; }
}

export function ReleaseNotes({ notes, onError }: { notes: string | null; onError: (error: AppError) => void }) {
  const text = (notes ?? "").slice(0, NOTES_LIMIT);
  const link = (href: string | undefined, children: React.ReactNode) => {
    const safe = href && safeWebUrl(href);
    return safe ? <a href={safe} onClick={e => {
      e.preventDefault();
      void callCommand("open_update_link", { url: safe }).catch(error => onError(normalizeAppError(error)));
    }} onAuxClick={e => e.preventDefault()}>{children}</a> : <span>{children}</span>;
  };
  return <div className="release-notes">
    {!text.trim() ? <p>此版本未提供更新说明，可在发布页查看详情。</p> :
      <NotesBoundary key={text} text={text}>
        <Markdown remarkPlugins={[remarkGfm]} skipHtml urlTransform={url => safeWebUrl(url) ?? ""}
          components={{
            a: ({ href, children }) => link(href, children),
            img: ({ src, alt }) => <span className="release-image-link">{link(src, `图片：${alt || "查看图片"}`)}</span>,
            input: ({ checked }) => <input type="checkbox" disabled readOnly checked={!!checked} aria-label={checked ? "已完成" : "未完成"} />,
            table: ({ children }) => <div className="release-table-scroll" tabIndex={0} role="region" aria-label="更新说明表格"><table>{children}</table></div>,
          }}>{text}</Markdown>
      </NotesBoundary>}
    {(notes?.length ?? 0) > NOTES_LIMIT && <p className="settings-field-hint">更新说明过长，已显示前 50,000 个字符。完整内容请查看发布页。</p>}
  </div>;
}
