import type { FsNode } from "@/data/mockDrive";

// Data-visualisation palette (canvas needs concrete colors; mirrors chart tokens in styles.css).
export type ColorMode = "ext" | "age" | "junk";

export const EXT_CATS = [
  { key: "video", label: "Video & Media", color: "#2DD4BF", exts: ["mp4", "mov", "mkv", "mp3", "flac"] },
  { key: "archive", label: "Archives & Images", color: "#F5B83D", exts: ["zip", "7z", "rar", "tar", "iso", "gz"] },
  { key: "code", label: "Code & Projects", color: "#22C55E", exts: ["md", "ts", "rs", "py", "js", "json", "safetensors"] },
  { key: "binary", label: "Binaries & Installers", color: "#7C93B8", exts: ["exe", "msi", "dll", ""] },
  { key: "system", label: "System", color: "#EF6F4F", exts: ["sys"] },
  { key: "folder", label: "Folders", color: "#3E5A4B", exts: [] },
] as const;

export function extColor(n: FsNode) {
  if (n.extension === "sys" || /windows|system/i.test(n.path) ) return EXT_CATS[4].color;
  if (n.isDir) return n.junkTag ? EXT_CATS[2].color : EXT_CATS[5].color;
  return EXT_CATS.find((c) => (c.exts as readonly string[]).includes(n.extension))?.color ?? EXT_CATS[3].color;
}

export const AGE_STOPS = [
  { label: "< 30d", color: "#22C55E", max: 30 },
  { label: "< 6m", color: "#2BA89A", max: 180 },
  { label: "< 1y", color: "#64748B", max: 365 },
  { label: "< 2y", color: "#E0A13A", max: 730 },
  { label: "> 2y", color: "#F0694A", max: Infinity },
];
export const ageColor = (n: FsNode) => (AGE_STOPS.find((s) => n.lastModifiedDaysAgo < s.max) ?? AGE_STOPS[4]!).color;

export function containsJunk(n: FsNode): boolean {
  return !!n.junkTag || !!n.children?.some(containsJunk);
}
export const junkColor = (n: FsNode) => (n.junkTag ? "#1ED760" : containsJunk(n) ? "#1F5E3A" : "#232927");

export function colorFor(n: FsNode, mode: ColorMode) {
  return mode === "ext" ? extColor(n) : mode === "age" ? ageColor(n) : junkColor(n);
}

export function legendFor(mode: ColorMode) {
  if (mode === "ext") return EXT_CATS.map((c) => ({ label: c.label, color: c.color }));
  if (mode === "age") return AGE_STOPS.map((s) => ({ label: s.label, color: s.color }));
  return [{ label: "Dev artifact", color: "#1ED760" }, { label: "Contains artifacts", color: "#1F5E3A" }, { label: "Other", color: "#232927" }];
}
