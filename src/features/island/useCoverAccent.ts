import { useEffect, useState } from "react";
import { accentFromImage } from "./coverAccent";

export function useCoverAccent(url: string | null): string | undefined {
  const [result, setResult] = useState<{ url: string; color?: string }>();
  useEffect(() => {
    if (!url) return;
    let disposed = false;
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => {
      if (disposed) return;
      let color: string | undefined;
      try { color = accentFromImage(image); } catch { /* Unreadable/CORS-blocked covers use the theme. */ }
      setResult({ url, color });
    };
    image.onerror = () => { if (!disposed) setResult({ url }); };
    image.src = url;
    return () => { disposed = true; image.onload = null; image.onerror = null; };
  }, [url]);
  // A delayed image from the previous work must never tint the current work.
  return url && result?.url === url ? result.color : undefined;
}
