export type JunkTag = "node_modules" | "cargo_target" | "python_venv" | "docker_cache" | "gradle" | null;

export interface FsNode {
  id: string;
  name: string;
  path: string;
  sizeBytes: number;
  deltaBytes: number;
  isDir: boolean;
  fileCount: number;
  lastModifiedDaysAgo: number;
  extension: string;
  junkTag: JunkTag;
  safetyHint: string;
  children?: FsNode[];
}

const GB = 1024 ** 3;
const MB = 1024 ** 2;
let seq = 0;

type Opts = Partial<Pick<FsNode, "deltaBytes" | "junkTag" | "safetyHint" | "fileCount">>;

function file(name: string, bytes: number, days: number, o: Opts = {}): FsNode {
  const dot = name.lastIndexOf(".");
  return {
    id: `n${++seq}`, name, path: "", sizeBytes: Math.round(bytes), deltaBytes: o.deltaBytes ?? 0,
    isDir: false, fileCount: 1, lastModifiedDaysAgo: days,
    extension: dot > 0 ? name.slice(dot + 1).toLowerCase() : "",
    junkTag: o.junkTag ?? null, safetyHint: o.safetyHint ?? hintFor(name),
  };
}

/** Opaque folder (aggregated contents, not expanded). */
function blob(name: string, bytes: number, files: number, days: number, o: Opts = {}): FsNode {
  return { ...file(name, bytes, days, o), isDir: true, fileCount: files, extension: "", children: [] , safetyHint: o.safetyHint ?? hintFor(name, o.junkTag ?? null)};
}

function dir(name: string, children: FsNode[], o: Opts = {}): FsNode {
  const n: FsNode = {
    id: `n${++seq}`, name, path: "", sizeBytes: 0, deltaBytes: 0, isDir: true, fileCount: 0,
    lastModifiedDaysAgo: 0, extension: "", junkTag: o.junkTag ?? null,
    safetyHint: o.safetyHint ?? hintFor(name, o.junkTag ?? null), children,
  };
  return n;
}

export function hintFor(name: string, tag: JunkTag = null): string {
  switch (tag) {
    case "node_modules": return "Safe to delete — reinstall anytime with `npm install` / `pnpm install`.";
    case "cargo_target": return "Safe to delete — Cargo build cache will regenerate automatically on next build.";
    case "python_venv": return "Safe to delete — recreate with `python -m venv .venv && pip install -r requirements.txt`.";
    case "docker_cache": return "Safe to delete — run `docker builder prune` to reclaim; images re-pull on demand.";
    case "gradle": return "Safe to delete — Gradle re-downloads dependencies on next sync.";
  }
  const l = name.toLowerCase();
  if (l === "hiberfil.sys") return "SYSTEM: Do not delete manually — disable hibernation via `powercfg -h off` to reclaim this space.";
  if (l === "pagefile.sys") return "SYSTEM: Managed by Windows virtual memory. Resize via System Properties → Advanced.";
  if (l.includes("windows") || l === "system32" || l === "winsxs") return "SYSTEM: Do not delete. Use Disk Cleanup → 'Clean up system files' instead.";
  if (l.endsWith(".iso")) return "Usually safe — installer images can be re-downloaded.";
  if (l.endsWith(".zip") || l.endsWith(".7z") || l.endsWith(".rar")) return "Check contents first — archives may be your only copy.";
  if (l.includes("temp") || l.includes("cache")) return "Safe to delete — temporary data is recreated as needed.";
  if (l.endsWith(".mp4") || l.endsWith(".mov") || l.endsWith(".mkv")) return "Personal media — ensure a backup exists before deleting.";
  return "Review before deleting — no automatic recovery hint available.";
}

function project(name: string, days: number, junk: { tag: Exclude<JunkTag, null>; gb: number }, srcMb: number): FsNode {
  const junkName = { node_modules: "node_modules", cargo_target: "target", python_venv: ".venv", docker_cache: ".docker-cache", gradle: ".gradle" }[junk.tag];
  const files = Math.round(junk.gb * 9000);
  return dir(name, [
    blob(junkName, junk.gb * GB, files, days, { junkTag: junk.tag, deltaBytes: days < 3 ? junk.gb * 0.08 * GB : 0 }),
    blob("src", srcMb * MB, Math.round(srcMb * 12), days),
    blob(".git", srcMb * 0.6 * MB, Math.round(srcMb * 4), days),
    file("README.md", 6200, days + 3),
  ]);
}

function buildTree(): FsNode {
  const projects = dir("Projects", [
    project("nebula-dashboard", 1, { tag: "node_modules", gb: 3.8 }, 42),
    project("rust-raytracer", 74, { tag: "cargo_target", gb: 6.4 }, 18),
    project("ml-pipeline", 128, { tag: "python_venv", gb: 4.1 }, 66),
    project("tauri-notes", 2, { tag: "cargo_target", gb: 3.2 }, 24),
    project("shop-storefront", 210, { tag: "node_modules", gb: 2.1 }, 35),
    project("android-companion", 96, { tag: "gradle", gb: 2.6 }, 51),
    project("api-gateway", 12, { tag: "docker_cache", gb: 3.4 }, 29),
    project("blog-astro", 340, { tag: "node_modules", gb: 1.2 }, 12),
    project("wasm-synth", 66, { tag: "cargo_target", gb: 2.9 }, 9),
    project("data-scraper", 412, { tag: "python_venv", gb: 1.6 }, 7),
    project("design-tokens", 30, { tag: "node_modules", gb: 0.9 }, 4),
    project("game-jam-2025", 280, { tag: "node_modules", gb: 1.8 }, 140),
    project("cli-toolkit", 5, { tag: "cargo_target", gb: 2.3 }, 11),
    project("legacy-crm", 940, { tag: "node_modules", gb: 2.1 }, 88),
  ]);
  const downloads = dir("Downloads", [
    file("ubuntu-24.04.iso", 5.7 * GB, 188),
    file("Win11_23H2_x64.iso", 6.2 * GB, 402),
    file("dataset-imagenet-subset.tar", 11.4 * GB, 0, { deltaBytes: 11.4 * GB }),
    file("Figma-Setup.exe", 210 * MB, 41),
    file("photos-backup-2023.zip", 8.3 * GB, 560),
    file("conference-talk.mp4", 2.4 * GB, 92),
    file("node-v22.11.0-x64.msi", 31 * MB, 30),
    file("OBS-Studio-30.zip", 380 * MB, 120),
  ]);
  const appData = dir("AppData", [
    dir("Local", [
      blob("Docker", 28.6 * GB, 4100, 0, { junkTag: "docker_cache", deltaBytes: 4.2 * GB }),
      blob("Temp", 6.8 * GB, 48200, 0, { deltaBytes: 1.1 * GB }),
      blob("Google Chrome Cache", 3.2 * GB, 21000, 0, { deltaBytes: 180 * MB }),
      blob("npm-cache", 4.4 * GB, 260000, 1, { deltaBytes: 220 * MB }),
      blob("JetBrains", 5.1 * GB, 30100, 14),
    ]),
    dir("Roaming", [
      blob("Code", 2.3 * GB, 18000, 0),
      blob("Spotify", 3.6 * GB, 900, 2),
      blob("discord", 1.1 * GB, 2300, 3),
    ]),
  ]);
  const users = dir("Users", [
    dir("dev", [projects, downloads, appData,
      dir("Documents", [blob("Invoices", 420 * MB, 680, 60), blob("Thesis", 1.8 * GB, 320, 720), file("tax-2025.pdf", 14 * MB, 200)]),
      dir("Pictures", [blob("Camera Roll", 18.4 * GB, 9200, 8), blob("Screenshots", 2.2 * GB, 4100, 0, { deltaBytes: 40 * MB })]),
    ]),
    dir("Public", [blob("Shared", 340 * MB, 120, 600)]),
  ]);
  const games = dir("Games", [
    dir("SteamLibrary", [
      dir("Cyberpunk2077", [file("archive", 61.2 * GB, 22, { safetyHint: "Game data — uninstall through Steam to avoid a broken install." }), blob("bin", 1.4 * GB, 340, 22)]),
      blob("Baldur's Gate 3", 75.5 * GB, 6100, 310, { safetyHint: "Untouched for 10 months — uninstall through Steam to reclaim." }),
      blob("Counter-Strike 2", 33.1 * GB, 2400, 0, { deltaBytes: 2.6 * GB }),
      blob("Hades II", 11.2 * GB, 900, 45),
      blob("shadercache", 5.6 * GB, 3100, 0, { safetyHint: "Safe to delete — shaders rebuild on next launch (first launch will stutter).", deltaBytes: 600 * MB }),
    ]),
  ]);
  const windows = dir("Windows", [
    dir("System", [blob("System32", 9.8 * GB, 21000, 6), blob("WinSxS", 14.2 * GB, 68000, 6, { deltaBytes: 900 * MB }), blob("SoftwareDistribution", 3.1 * GB, 1900, 4)]),
    blob("Installer", 6.4 * GB, 1200, 30),
    blob("Fonts", 1.2 * GB, 1400, 200),
  ]);
  const media = dir("Media", [
    dir("Videos", [
      file("wedding-raw-4k.mov", 22.6 * GB, 830),
      file("drone-iceland.mp4", 9.4 * GB, 480),
      file("screen-recording-sprint.mkv", 4.8 * GB, 0, { deltaBytes: 4.8 * GB }),
      file("family-2019.mp4", 3.1 * GB, 940),
    ]),
    blob("Music", 14.6 * GB, 5200, 300),
  ]);
  const root = dir("C:\\", [users, games, windows, media,
    file("hiberfil.sys", 12.8 * GB, 0),
    file("pagefile.sys", 16 * GB, 0),
    blob("Program Files", 24.3 * GB, 81000, 18),
  ]);
  return finalize(root, "C:");
}

export function finalize(n: FsNode, parentPath: string): FsNode {
  const path = n.name === "C:\\" ? "C:\\" : `${parentPath.replace(/\\$/, "")}\\${n.name}`;
  if (!n.isDir || !n.children || n.children.length === 0) return { ...n, path };
  const children = n.children.map((c) => finalize(c, path));
  return {
    ...n, path, children,
    sizeBytes: children.reduce((a, c) => a + c.sizeBytes, 0),
    deltaBytes: children.reduce((a, c) => a + c.deltaBytes, 0),
    fileCount: children.reduce((a, c) => a + c.fileCount, 0),
    lastModifiedDaysAgo: Math.min(...children.map((c) => c.lastModifiedDaysAgo)),
  };
}

/** Remove a node by id and recompute ancestor aggregates. */
export function removeNode(root: FsNode, id: string): FsNode {
  const strip = (n: FsNode): FsNode => {
    if (!n.children || n.children.length === 0) return n;
    const kids = n.children.filter((c) => c.id !== id).map(strip);
    return {
      ...n, children: kids,
      sizeBytes: kids.reduce((a, c) => a + c.sizeBytes, 0),
      deltaBytes: kids.reduce((a, c) => a + c.deltaBytes, 0),
      fileCount: kids.reduce((a, c) => a + c.fileCount, 0),
    };
  };
  return strip(root);
}

export function findPath(root: FsNode, id: string): FsNode[] | null {
  if (root.id === id) return [root];
  for (const c of root.children ?? []) {
    const p = findPath(c, id);
    if (p) return [root, ...p];
  }
  return null;
}

export function flatten(root: FsNode): FsNode[] {
  const out: FsNode[] = [];
  const walk = (n: FsNode) => { out.push(n); n.children?.forEach(walk); };
  walk(root);
  return out;
}

export const initialDrive = buildTree();
export const DRIVE_CAPACITY = 1024 * GB;
export const TOTAL_FILES = 842190;

export interface DupGroup { id: string; name: string; hash: string; sizeBytes: number; copies: { path: string; daysAgo: number }[] }

export const duplicateGroups: DupGroup[] = [
  { id: "d1", name: "photos-backup-2023.zip", hash: "b3:9f82a4…c10e", sizeBytes: 1.5 * GB, copies: [
    { path: "C:\\Users\\dev\\Downloads\\photos-backup-2023.zip", daysAgo: 560 },
    { path: "C:\\Users\\dev\\Documents\\Backups\\photos-backup-2023.zip", daysAgo: 410 },
    { path: "C:\\Media\\Archive\\photos-backup-2023 (1).zip", daysAgo: 12 }] },
  { id: "d2", name: "ubuntu-24.04.iso", hash: "b3:41ce07…8a2f", sizeBytes: 5.7 * GB, copies: [
    { path: "C:\\Users\\dev\\Downloads\\ubuntu-24.04.iso", daysAgo: 188 },
    { path: "C:\\VMs\\isos\\ubuntu-24.04.iso", daysAgo: 150 }] },
  { id: "d3", name: "conference-talk.mp4", hash: "b3:c2d9e1…04b7", sizeBytes: 2.4 * GB, copies: [
    { path: "C:\\Users\\dev\\Downloads\\conference-talk.mp4", daysAgo: 92 },
    { path: "C:\\Media\\Videos\\talks\\conference-talk.mp4", daysAgo: 90 }] },
  { id: "d4", name: "Figma-Setup.exe", hash: "b3:7a10bb…e93d", sizeBytes: 210 * MB, copies: [
    { path: "C:\\Users\\dev\\Downloads\\Figma-Setup.exe", daysAgo: 41 },
    { path: "C:\\Users\\dev\\Downloads\\Figma-Setup (1).exe", daysAgo: 40 },
    { path: "C:\\Users\\dev\\Downloads\\Figma-Setup (2).exe", daysAgo: 3 }] },
  { id: "d5", name: "model-weights-v3.safetensors", hash: "b3:e5f601…7d21", sizeBytes: 1.6 * GB, copies: [
    { path: "C:\\Users\\dev\\Projects\\ml-pipeline\\models\\model-weights-v3.safetensors", daysAgo: 128 },
    { path: "C:\\Users\\dev\\.cache\\huggingface\\model-weights-v3.safetensors", daysAgo: 130 }] },
  { id: "d6", name: "drone-iceland.mp4", hash: "b3:0b3a9c…f5e8", sizeBytes: 1.08 * GB, copies: [
    { path: "C:\\Media\\Videos\\drone-iceland.mp4", daysAgo: 480 },
    { path: "C:\\Users\\dev\\Desktop\\drone-iceland-export.mp4", daysAgo: 470 }] },
];
