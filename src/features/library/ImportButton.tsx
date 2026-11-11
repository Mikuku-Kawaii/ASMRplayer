import { useId, useRef, useState } from "react";
import { ChevronDown, FolderPlus, Library } from "lucide-react";
import { ContextMenu } from "../../components/ContextMenu";

export function ImportButton({ onChoose, disabled = false }: {
  onChoose: (mode: "single_work" | "library_root") => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  const choose = (mode: "single_work" | "library_root") => { setOpen(false); onChoose(mode); };
  return <div className="btn-split">
    <button type="button" className="btn btn-primary" disabled={disabled}
      aria-label="导入作品文件夹" title="选择一个作品文件夹并预览导入"
      onClick={() => choose("single_work")}>
      <FolderPlus aria-hidden="true" /><span className="ui-icon-label">导入作品</span>
    </button>
    <button ref={trigger} type="button" className="btn btn-primary" disabled={disabled}
      title="选择导入方式" aria-label="选择导入方式" aria-haspopup="menu"
      aria-expanded={open} aria-controls={open ? id : undefined}
      onClick={() => setOpen(value => !value)}
      onKeyDown={event => { if (event.key === "ArrowDown") { event.preventDefault(); setOpen(true); } }}>
      <ChevronDown aria-hidden="true" />
    </button>
    {open && <ContextMenu id={id} x={0} y={0} trigger={trigger.current ?? undefined}
      label="导入方式" onClose={() => setOpen(false)}>
      <button type="button" role="menuitem" className="m3-menu-item" onClick={() => choose("single_work")}>
        <FolderPlus aria-hidden="true" /><span className="import-menu-copy"><span>导入作品文件夹</span><small>将所选文件夹作为一部作品</small></span>
      </button>
      <button type="button" role="menuitem" className="m3-menu-item" onClick={() => choose("library_root")}>
        <Library aria-hidden="true" /><span className="import-menu-copy"><span>导入作品库根目录</span><small>扫描子文件夹，批量导入多部作品</small></span>
      </button>
    </ContextMenu>}
  </div>;
}
