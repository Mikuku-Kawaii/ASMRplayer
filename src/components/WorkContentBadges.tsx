export function WorkContentBadges({ nsfw, hasSubtitle, hasRecognizedSubtitle, overlay = true }: {
  nsfw?: boolean; hasSubtitle?: boolean; hasRecognizedSubtitle?: boolean; overlay?: boolean;
}) {
  if (!nsfw && !hasSubtitle && !hasRecognizedSubtitle) return null;
  return <div className={overlay ? "work-card-cover-badges" : "work-badges"}>
    {(hasSubtitle || hasRecognizedSubtitle) && <span className="badge badge-downloaded"><span className="ui-tag-label">{hasRecognizedSubtitle ? "识别字幕" : "含字幕"}</span></span>}
    {nsfw && <span className="badge badge-nsfw"><span className="ui-tag-label">R18</span></span>}
  </div>;
}
