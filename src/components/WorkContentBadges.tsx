export function WorkContentBadges({ nsfw, hasSubtitle, overlay = true }: {
  nsfw?: boolean; hasSubtitle?: boolean; overlay?: boolean;
}) {
  if (!nsfw && !hasSubtitle) return null;
  return <div className={overlay ? "work-card-cover-badges" : "work-badges"}>
    {hasSubtitle && <span className="badge badge-downloaded"><span className="ui-tag-label">含字幕</span></span>}
    {nsfw && <span className="badge badge-nsfw"><span className="ui-tag-label">R18</span></span>}
  </div>;
}
