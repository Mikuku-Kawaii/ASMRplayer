import { useEffect, useState } from "react";
import type { OnlineFilterState } from "../features/online/OnlineView";
import type { LibraryCollectionState } from "../features/library/SmartCollections";

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const isStringArray = (value: unknown): value is string[] =>
  Array.isArray(value) && value.every(item => typeof item === "string");

// Read synchronously before rendering a page, so its first request uses saved
// conditions. Empty strings/arrays and false are saved values, not defaults.
export function useSavedState<T>(key: string, initial: T, valid: (value: unknown) => boolean) {
  const storageKey = `asmr-player.page-preferences.v1.${key}`;
  const [value, setValue] = useState<T>(() => {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "null");
      return valid(saved) ? saved as T : initial;
    } catch { return initial; }
  });
  useEffect(() => {
    try { localStorage.setItem(storageKey, JSON.stringify(value)); }
    catch { /* Keep page navigation usable when WebView storage is unavailable. */ }
  }, [storageKey, value]);
  return [value, setValue] as const;
}

export function usePagePreferences() {
  const [libraryCollection, setLibraryCollection] = useSavedState<LibraryCollectionState>(
    "library-collection", { filters: null, selectedId: "" },
    value => {
      if (!isRecord(value) || typeof value.selectedId !== "string") return false;
      const filters = value.filters;
      return filters === null || (isRecord(filters) && typeof filters.keyword === "string"
        && isStringArray(filters.voiceActors) && isStringArray(filters.tags)
        && ["collected", "favorite", "offline"].every(key => filters[key] === null || typeof filters[key] === "boolean")
        && ["any", "unplayed", "inProgress", "played"].includes(String(filters.listening)));
    },
  );
  const [onlinePage, setOnlinePage] = useSavedState("online-page", 1, value => Number.isSafeInteger(value) && Number(value) >= 1);
  const [settingsSection, setSettingsSection] = useSavedState("settings-section", "privacy-window", value => typeof value === "string");
  const [visibleCounts, setVisibleCounts] = useSavedState<Record<string, number>>("visible-counts", {}, value => isRecord(value)
    && Object.values(value).every(count => Number.isSafeInteger(count) && Number(count) >= 1));
  const [pageSearches, setPageSearches] = useSavedState(
    "searches", { library: "", favorites: "", recent: "", settings: "" },
    value => isRecord(value) && ["library", "favorites", "recent", "settings"].every(key => typeof value[key] === "string"),
  );
  const [onlineSearch, setOnlineSearch] = useSavedState(
    "online-search", { keyword: "", committedKeyword: "" },
    value => isRecord(value) && typeof value.keyword === "string" && typeof value.committedKeyword === "string",
  );
  const [pageFilters, setPageFilters] = useSavedState<Record<"library" | "favorites" | "recent", string[]>>(
    "filters", { library: [], favorites: [], recent: [] },
    value => isRecord(value) && ["library", "favorites", "recent"].every(key => isStringArray(value[key])),
  );
  const [onlineFilters, setOnlineFilters] = useSavedState<OnlineFilterState>(
    "online-filters", { order: "popular", sort: "desc", subtitleOnly: false, filters: [] },
    value => isRecord(value) && typeof value.order === "string" && typeof value.sort === "string"
      && typeof value.subtitleOnly === "boolean" && Array.isArray(value.filters)
      && value.filters.every(filter => isRecord(filter) && ["voiceActor", "circle", "tag", "age"].includes(String(filter.kind))
        && typeof filter.value === "string" && typeof filter.exclude === "boolean"),
  );
  return { libraryCollection, setLibraryCollection, onlinePage, setOnlinePage, settingsSection, setSettingsSection, visibleCounts, setVisibleCounts,
    pageSearches, setPageSearches, onlineSearch, setOnlineSearch, pageFilters, setPageFilters, onlineFilters, setOnlineFilters };
}
