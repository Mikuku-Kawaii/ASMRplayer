import { useRef, useState } from "react";
import { Trash2 } from "lucide-react";
import { AppError, callCommand, LibraryRootDto, normalizeAppError } from "../../lib/api";

export function RemoveLibraryRootButton({ root, onRemoved, onError }: {
  root: LibraryRootDto; onRemoved: (root: LibraryRootDto) => void; onError: (error: AppError) => void;
}) {
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  async function remove() {
    if (busy.current) return;
    busy.current = true; setPending(true);
    try {
      await callCommand("remove_library_root", { rootId: root.id });
      onRemoved(root);
    } catch (error) { onError(normalizeAppError(error)); }
    finally { busy.current = false; setPending(false); }
  }
  return <button type="button" className="btn btn-ghost btn-sm" disabled={pending} onClick={() => void remove()}
    aria-label={`移除目录引用：${root.displayName}`}
    title="移除该目录的本地来源并停止监听；无其他来源的作品移出音频库，保留收听记录和磁盘文件">
    <Trash2 size={13} aria-hidden="true" /><span className="ui-icon-label">{pending ? "正在移除…" : "移除目录引用"}</span>
  </button>;
}
