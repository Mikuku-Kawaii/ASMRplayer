import { ReactNode } from "react";
import { ContentDialog } from "../../components/ContentDialog";

export function ImportDialog(props: { children: ReactNode; onClose: () => void; busy: boolean }) {
  return <ContentDialog {...props} label="本地目录导入预览" />;
}
