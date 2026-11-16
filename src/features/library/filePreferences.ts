import type { DirectoryGroupOption, TrackItemDto } from "../../lib/api";

export interface FilePreferences { effects: "any" | "with" | "without"; format: "any" | "mp3" | "wav" }
export const defaultFilePreferences: FilePreferences = { effects: "any", format: "any" };
export function normalizeFilePreferences(value?: Partial<FilePreferences> | null): FilePreferences {
  return { effects: value?.effects === "with" || value?.effects === "without" ? value.effects : "any",
    format: value?.format === "mp3" || value?.format === "wav" ? value.format : "any" };
}

const noEffects = /se(?:なし|無し|ナシ|無)|[无無]\s*(?:se|效果音|音效|効果音)|(?:効果音|效果音|音效)(?:なし|無し|ナシ)|(?<![a-z])(?:no|without)[\s_.-]*se(?:$|[^a-z])|なし版|無し版/;
const withEffects = /se(?:あり|有り|アリ|有)|[有含带帶]\s*(?:se|效果音|音效|効果音)|(?:効果音|效果音|音效)(?:あり|有り|アリ)|(?<![a-z])with[\s_.-]*se(?:$|[^a-z])|あり版|有り版/;
const supplementary = /試聽|试听|試聴|体験版|體驗版|サンプル|過往作品|过往作品|特典|おまけ|额外|額外|フリートーク|自由谈话|自由談話|ng片段|ng集|予告|预告|預告|(?<![a-z])(?:sample|preview|trailer|bonus|extra|free[ _-]*talk)(?![a-z])/;

export function directoryEffects(path: string): FilePreferences["effects"] {
  for (const segment of path.normalize("NFKC").toLowerCase().split(/[/\\]/).reverse()) {
    const text = segment.replace(/(se|効果音|效果音|音效)\s+/g, "$1");
    if (noEffects.test(text)) return "without";
    if (withEffects.test(text)) return "with";
  }
  return "any";
}

function preferenceScores(groups: DirectoryGroupOption[], preferences: FilePreferences) {
  const paths = groups.map(group => group.groupPath.replace(/\\/g, "/").split("/"));
  let prefix = 0;
  while (paths.length && paths.every(path => path.length > prefix && path[prefix] === paths[0]![prefix])) prefix++;
  return new Map(groups.map((group, index) => {
    const effects = directoryEffects(group.groupPath);
    const effectScore = preferences.effects === "any" ? 0 : effects === preferences.effects ? 4 : effects === "any" ? 2 : 0;
    const formatScore = preferences.format !== "any" && group.formats.some(format => format.trim().replace(/^\./, "").toLowerCase() === preferences.format) ? 1 : 0;
    const mainScore = supplementary.test(paths[index]!.slice(prefix).join("/").toLowerCase()) ? 0 : 8;
    return [group.groupPath, mainScore + effectScore + formatScore];
  }));
}

export function preferredDirectoryOrder(groups: DirectoryGroupOption[], preferences: FilePreferences) {
  const scores = preferenceScores(groups, preferences);
  return [...groups].sort((a, b) => scores.get(b.groupPath)! - scores.get(a.groupPath)! || directoryDepth(a.groupPath) - directoryDepth(b.groupPath));
}

function directoryDepth(path: string) { return path.split(/[/\\]/).filter(Boolean).length; }

export function preferredDirectory(groups: DirectoryGroupOption[], preferences: FilePreferences, resume?: string | null): string {
  const sorted = preferredDirectoryOrder(groups, preferences);
  const best = sorted[0];
  if (!best) return "";
  const scores = preferenceScores(groups, preferences);
  return resume != null && scores.has(resume) && scores.get(resume) === scores.get(best.groupPath) && directoryDepth(resume) === directoryDepth(best.groupPath) ? resume : best.groupPath;
}

export function resumeDirectory(tracks: TrackItemDto[], lastVersionId?: string | null) {
  return tracks.find(track => track.versions.some(version => version.id === lastVersionId && version.positionMs > 0 && !version.completed))?.logicalDirPath;
}
