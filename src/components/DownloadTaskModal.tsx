import React, { useMemo, useState, useEffect } from "react";
import { createPortal } from "react-dom";
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FileAudio,
  FileText,
  FileImage,
  Check,
  Minus,
  X,
} from "lucide-react";
import {
  DownloadPreviewDto,
  DownloadPreviewItemDto,
  formatBytes,
} from "../lib/api";

interface DownloadTaskModalProps {
  preview: DownloadPreviewDto;
  selectedIds: Record<string, boolean>;
  onChangeSelectedIds: React.Dispatch<
    React.SetStateAction<Record<string, boolean>>
  >;
  onConfirm: () => void;
  onClose: () => void;
}

interface TreeFileNode {
  kind: "file";
  key: string;
  name: string;
  ext: string;
  byteSize: number;
  item: DownloadPreviewItemDto;
}

interface TreeFolderNode {
  kind: "folder";
  key: string;
  name: string;
  folders: TreeFolderNode[];
  files: TreeFileNode[];
  totalBytes: number;
  downloadableSourceIds: string[];
}

interface MutableFolderBuilder {
  key: string;
  name: string;
  subfolders: Map<string, MutableFolderBuilder>;
  files: TreeFileNode[];
}

function buildDownloadTree(
  items: DownloadPreviewItemDto[],
  fallbackFolderName: string
): TreeFolderNode[] {
  const rootMap = new Map<string, MutableFolderBuilder>();

  for (const item of items) {
    const ext = (item.format || "mp3").toLowerCase();
    const rawRel = (item.targetRelativePath || item.trackTitle).replace(
      /\\/g,
      "/"
    );
    const parts = rawRel.split("/").filter(Boolean);

    const rawLeafName = parts[parts.length - 1] ?? item.trackTitle;
    const hasExt = rawLeafName.toLowerCase().endsWith(`.${ext}`);
    const fileName = hasExt ? rawLeafName : `${rawLeafName}.${ext}`;
    const folderSegments =
      parts.length > 1 ? parts.slice(0, -1) : [fallbackFolderName || "根目录"];

    let currentMap = rootMap;
    let currentBuilder: MutableFolderBuilder | null = null;
    let pathAcc = "";

    for (const seg of folderSegments) {
      pathAcc = pathAcc ? `${pathAcc}/${seg}` : seg;
      let nextBuilder = currentMap.get(seg);
      if (!nextBuilder) {
        nextBuilder = {
          key: pathAcc,
          name: seg,
          subfolders: new Map(),
          files: [],
        };
        currentMap.set(seg, nextBuilder);
      }
      currentBuilder = nextBuilder;
      currentMap = nextBuilder.subfolders;
    }

    const fileNode: TreeFileNode = {
      kind: "file",
      key: item.sourceId,
      name: fileName,
      ext,
      byteSize: item.byteSize ?? 0,
      item,
    };

    if (currentBuilder) {
      currentBuilder.files.push(fileNode);
    }
  }

  function finalizeFolder(b: MutableFolderBuilder): TreeFolderNode {
    const folders = Array.from(b.subfolders.values()).map(finalizeFolder);
    const files = b.files;

    let totalBytes = 0;
    const downloadableSourceIds: string[] = [];

    for (const sub of folders) {
      totalBytes += sub.totalBytes;
      downloadableSourceIds.push(...sub.downloadableSourceIds);
    }
    for (const f of files) {
      totalBytes += f.byteSize;
      if (!f.item.alreadyDownloaded && !f.item.alreadyQueued) {
        downloadableSourceIds.push(f.item.sourceId);
      }
    }

    return {
      kind: "folder",
      key: b.key,
      name: b.name,
      folders,
      files,
      totalBytes,
      downloadableSourceIds,
    };
  }

  return Array.from(rootMap.values()).map(finalizeFolder);
}

function renderFileTypeIcon(ext: string) {
  if (["mp3", "wav", "flac", "m4a", "ogg", "opus", "aac"].includes(ext)) {
    return (
      <span className="dl-file-icon dl-file-icon-audio" aria-hidden="true">
        <FileAudio size={14} />
      </span>
    );
  }
  if (["jpg", "jpeg", "png", "webp", "gif", "bmp"].includes(ext)) {
    return (
      <span className="dl-file-icon dl-file-icon-image" aria-hidden="true">
        <FileImage size={14} />
      </span>
    );
  }
  return (
    <span className="dl-file-icon dl-file-icon-doc" aria-hidden="true">
      <FileText size={14} />
    </span>
  );
}

export const DownloadTaskModal: React.FC<DownloadTaskModalProps> = ({
  preview,
  selectedIds,
  onChangeSelectedIds,
  onConfirm,
  onClose,
}) => {
  const [collapsedKeys, setCollapsedKeys] = useState<Record<string, boolean>>(
    {}
  );

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  // Lock .main-content scroll while download modal is open and restore on close
  useEffect(() => {
    const mainEl = document.querySelector<HTMLElement>(".main-content");
    const prevOverflow = mainEl?.style.overflow ?? "";
    const prevOverscroll = mainEl?.style.overscrollBehavior ?? "";
    const savedScrollTop = mainEl?.scrollTop ?? 0;

    if (mainEl) {
      mainEl.classList.add("is-scroll-locked");
      mainEl.style.overflow = "hidden";
      mainEl.style.overscrollBehavior = "contain";
    }
    document.body.classList.add("is-modal-scroll-locked");

    return () => {
      if (mainEl) {
        mainEl.classList.remove("is-scroll-locked");
        mainEl.style.overflow = prevOverflow;
        mainEl.style.overscrollBehavior = prevOverscroll;
        mainEl.scrollTop = savedScrollTop;
      }
      document.body.classList.remove("is-modal-scroll-locked");
    };
  }, []);

  const treeRoots = useMemo(
    () => buildDownloadTree(preview.items, preview.workTitle || "根目录"),
    [preview.items, preview.workTitle]
  );

  // Format chips list (lowercase, matching asmr.one style: mp3, vtt, wav, jpg, pdf, txt)
  const formatChips = useMemo(() => {
    const map = new Map<
      string,
      { ext: string; downloadableIds: string[]; totalCount: number }
    >();
    for (const item of preview.items) {
      const ext = (item.format || "mp3").toLowerCase();
      let entry = map.get(ext);
      if (!entry) {
        entry = { ext, downloadableIds: [], totalCount: 0 };
        map.set(ext, entry);
      }
      entry.totalCount += 1;
      if (!item.alreadyDownloaded && !item.alreadyQueued) {
        entry.downloadableIds.push(item.sourceId);
      }
    }
    return Array.from(map.values());
  }, [preview.items]);

  const selectedStats = useMemo(() => {
    let count = 0;
    let bytes = 0;
    for (const item of preview.items) {
      if (selectedIds[item.sourceId]) {
        count += 1;
        bytes += item.byteSize ?? 0;
      }
    }
    return { count, bytes };
  }, [preview.items, selectedIds]);

  const toggleFormatChip = (ext: string) => {
    const chip = formatChips.find((c) => c.ext === ext);
    if (!chip || chip.downloadableIds.length === 0) return;

    const allSelected = chip.downloadableIds.every((id) => selectedIds[id]);
    onChangeSelectedIds((prev) => {
      const next = { ...prev };
      for (const id of chip.downloadableIds) {
        next[id] = !allSelected;
      }
      return next;
    });
  };

  const toggleFolderCollapse = (folderKey: string) => {
    setCollapsedKeys((prev) => ({
      ...prev,
      [folderKey]: !prev[folderKey],
    }));
  };

  const toggleFolderSelection = (folder: TreeFolderNode) => {
    if (folder.downloadableSourceIds.length === 0) return;
    const allSelected = folder.downloadableSourceIds.every(
      (id) => selectedIds[id]
    );
    onChangeSelectedIds((prev) => {
      const next = { ...prev };
      for (const id of folder.downloadableSourceIds) {
        next[id] = !allSelected;
      }
      return next;
    });
  };

  const toggleFileSelection = (item: DownloadPreviewItemDto) => {
    if (item.alreadyDownloaded || item.alreadyQueued) return;
    onChangeSelectedIds((prev) => ({
      ...prev,
      [item.sourceId]: !prev[item.sourceId],
    }));
  };

  const renderFolderNode = (folder: TreeFolderNode, depth: number) => {
    const isCollapsed = Boolean(collapsedKeys[folder.key]);
    const downloadableCount = folder.downloadableSourceIds.length;
    const selectedInFolder = folder.downloadableSourceIds.filter(
      (id) => selectedIds[id]
    ).length;
    const isAllSelected =
      downloadableCount > 0 && selectedInFolder === downloadableCount;
    const isPartialSelected =
      selectedInFolder > 0 && selectedInFolder < downloadableCount;

    return (
      <div key={folder.key} className="dl-tree-group">
        <div
          className="dl-tree-row dl-tree-folder-row"
          style={{ paddingLeft: `${8 + depth * 18}px` }}
        >
          <button
            type="button"
            className="dl-tree-caret-btn"
            onClick={() => toggleFolderCollapse(folder.key)}
            aria-label={isCollapsed ? `展开 ${folder.name}` : `折叠 ${folder.name}`}
          >
            {isCollapsed ? (
              <ChevronRight size={14} />
            ) : (
              <ChevronDown size={14} />
            )}
          </button>

          <button
            type="button"
            role="checkbox"
            aria-checked={
              isPartialSelected ? "mixed" : isAllSelected ? "true" : "false"
            }
            aria-label={`选择文件夹 ${folder.name}`}
            disabled={downloadableCount === 0}
            className={`dl-tree-checkbox ${
              isAllSelected || isPartialSelected ? "is-checked" : ""
            }`}
            onClick={() => toggleFolderSelection(folder)}
          >
            {isAllSelected ? (
              <Check size={12} strokeWidth={3} />
            ) : isPartialSelected ? (
              <Minus size={12} strokeWidth={3} />
            ) : null}
          </button>

          <span
            className="dl-folder-icon"
            onClick={() => toggleFolderCollapse(folder.key)}
            aria-hidden="true"
          >
            <Folder size={15} fill="currentColor" />
          </span>

          <span
            className="dl-tree-node-name dl-folder-name"
            onClick={() => toggleFolderCollapse(folder.key)}
            title={folder.name}
          >
            {folder.name}
          </span>

          <span className="dl-tree-file-size tabular-nums">
            {formatBytes(folder.totalBytes)}
          </span>
        </div>

        {!isCollapsed && (
          <div className="dl-tree-children">
            {folder.folders.map((sub) => renderFolderNode(sub, depth + 1))}
            {folder.files.map((file) => renderFileNode(file, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  const renderFileNode = (file: TreeFileNode, depth: number) => {
    const { item } = file;
    const isDisabled = item.alreadyDownloaded || item.alreadyQueued;
    const isChecked = Boolean(selectedIds[item.sourceId]);

    return (
      <div
        key={file.key}
        className={`dl-tree-row dl-tree-file-row ${
          isDisabled ? "is-disabled" : ""
        }`}
        style={{ paddingLeft: `${24 + depth * 18}px` }}
        onClick={() => toggleFileSelection(item)}
      >
        <button
          type="button"
          role="checkbox"
          aria-checked={isChecked}
          aria-label={`选择文件 ${file.name}`}
          disabled={isDisabled}
          className={`dl-tree-checkbox ${isChecked ? "is-checked" : ""}`}
          onClick={(e) => {
            e.stopPropagation();
            toggleFileSelection(item);
          }}
        >
          {isChecked && <Check size={12} strokeWidth={3} />}
        </button>

        {renderFileTypeIcon(file.ext)}

        <span className="dl-tree-node-name" title={file.name}>
          {file.name}
        </span>

        {item.alreadyDownloaded ? (
          <span className="badge badge-downloaded">已下载</span>
        ) : item.alreadyQueued ? (
          <span className="badge badge-online">队列中</span>
        ) : (
          <span className="dl-tree-file-ext">{file.ext}</span>
        )}

        <span className="dl-tree-file-size tabular-nums">
          {formatBytes(file.byteSize)}
        </span>
      </div>
    );
  };

  const modalNode = (
    <div
      className="dl-task-popover-backdrop"
      onClick={onClose}
      onWheel={(e) => e.stopPropagation()}
      role="dialog"
      aria-modal="true"
      aria-label="创建下载任务"
    >
      <section
        className="dl-task-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="dl-task-header">
          <h3 className="dl-task-title">创建下载任务</h3>
          <button
            type="button"
            className="btn-icon drawer-close-btn"
            onClick={onClose}
            aria-label="关闭创建下载任务弹窗"
            title="关闭 (Esc)"
          >
            <X size={15} />
          </button>
        </header>

        {/* 顶部格式胶囊筛选 + 已选统计卡片 */}
        <div className="dl-task-summary-card">
          <div className="dl-format-chips-row">
            {formatChips.map((chip) => {
              const selectedInChip = chip.downloadableIds.filter(
                (id) => selectedIds[id]
              ).length;
              const isActive = selectedInChip > 0;
              return (
                <button
                  key={chip.ext}
                  type="button"
                  className={`dl-format-pill ${
                    isActive ? "is-active" : "is-inactive"
                  }`}
                  disabled={chip.downloadableIds.length === 0}
                  onClick={() => toggleFormatChip(chip.ext)}
                  aria-pressed={isActive}
                  title={`点击切换选择全部 ${chip.ext.toUpperCase()} 文件`}
                >
                  <span>{chip.ext}</span>
                </button>
              );
            })}
          </div>

          <div className="dl-summary-divider" />

          <div className="dl-selected-summary-row">
            <span className="dl-selected-summary tabular-nums">
              {selectedStats.count} 已选, 共 {formatBytes(selectedStats.bytes)}
            </span>
          </div>
        </div>

        {/* 中部可折叠层级文件夹与文件树 */}
        <div className="dl-tree-card" role="tree" aria-label="下载文件目录树">
          {treeRoots.length === 0 ? (
            <div className="empty-box">当前作品暂无可下载的在线文件</div>
          ) : (
            treeRoots.map((root) => renderFolderNode(root, 0))
          )}
        </div>

        {/* 底部操作栏 */}
        <footer className="dl-task-footer">
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={onClose}
          >
            <span>取消</span>
          </button>
          <button
            type="button"
            className="btn btn-primary btn-sm"
            disabled={selectedStats.count === 0}
            onClick={onConfirm}
          >
            <span>下载</span>
          </button>
        </footer>
      </section>
    </div>
  );

  return typeof document !== "undefined"
    ? createPortal(modalNode, document.body)
    : modalNode;
};
