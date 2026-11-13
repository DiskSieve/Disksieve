import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { ArrowUp, ChevronRight, Copy, File, Folder, FolderOpen, HardDrive, Search, ShieldAlert, ShieldCheck, RefreshCw, Trash2, Eye, ExternalLink } from "lucide-react";
import { DRIVE_CAPACITY, TOTAL_FILES, findPath, flatten, formatAge, formatBytes, initialDrive, removeNode, type FsNode } from "@/data/mockDrive";
import { legendFor, type ColorMode } from "@/lib/treemap-palette";
import { Logo } from "./Logo";
import { Treemap } from "./Treemap";
import { DevJunkView, type JunkRow } from "./DevJunkView";
import { DuplicatesView, initialDuplicates, wastedBytes } from "./DuplicatesView";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Progress } from "@/components/ui/progress";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@/components/ui/breadcrumb";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandShortcut } from "@/components/ui/command";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import { cn } from "@/lib/utils";

type Tab = "explorer" | "junk" | "dups";
const MODES: { key: ColorMode; label: string }[] = [{ key: "ext", label: "By Extension" }, { key: "age", label: "Cold Data (Age)" }, { key: "junk", label: "Dev Junk" }];

export function DiskSieveApp() {
  const [tree, setTree] = useState<FsNode>(initialDrive);
  const [currentId, setCurrentId] = useState(initialDrive.id);
  const [mode, setMode] = useState<ColorMode>("ext");
  const [delta, setDelta] = useState(false);
  const [tab, setTab] = useState<Tab>("explorer");
  const [inspectId, setInspectId] = useState<string | null>(null);
  const [ctxNode, setCtxNode] = useState<FsNode | null>(null);
  const [cmdOpen, setCmdOpen] = useState(false);
  const [dups, setDups] = useState(initialDuplicates);
  const [scan, setScan] = useState<{ pct: number; files: number } | null>(null);
  const [scanTime, setScanTime] = useState(1.42);

  const trail = useMemo(() => findPath(tree, currentId) ?? [tree], [tree, currentId]);
  const current = trail[trail.length - 1]!;
  const all = useMemo(() => flatten(tree), [tree]);
  const inspected = inspectId ? all.find((n) => n.id === inspectId) ?? null : null;
  const val = (n: FsNode) => (delta ? n.deltaBytes : n.sizeBytes);
  const items = useMemo(() => [...(current.children ?? [])].filter((n) => !delta || n.deltaBytes > 0).sort((a, b) => val(b) - val(a)), [current, delta]); // eslint-disable-line react-hooks/exhaustive-deps
  const folderTotal = items.reduce((a, n) => a + Math.max(val(n), 0), 0) || 1;
  const junkRows: JunkRow[] = useMemo(() => all.filter((n) => n.junkTag && n.path.includes("\\Projects\\")).map((n) => ({ node: n, project: n.path.split("\\").slice(-2)[0] ?? "" })), [all]);
  const junkTotal = junkRows.reduce((a, r) => a + r.node.sizeBytes, 0);

  const trash = useCallback((nodes: FsNode[]) => {
    setTree((prev) => {
      const next = nodes.reduce((t, n) => removeNode(t, n.id), prev);
      const bytes = nodes.reduce((a, n) => a + n.sizeBytes, 0);
      setTimeout(() => toast.success(nodes.length === 1 ? `Moved “${nodes[0]!.name}” to Trash` : `Moved ${nodes.length} items to Trash`, {
        description: `${formatBytes(bytes)} reclaimed`, action: { label: "Undo", onClick: () => setTree(prev) },
      }));
      return next;
    });
    setInspectId(null);
  }, []);

  const activate = (n: FsNode) => (n.isDir && n.children?.length ? setCurrentId(n.id) : setInspectId(n.id));

  const rescan = () => {
    if (scan) return;
    const t0 = performance.now();
    const tick = () => {
      const pct = Math.min(100, ((performance.now() - t0) / 800) * 100);
      setScan({ pct, files: Math.round((TOTAL_FILES * pct) / 100) });
      if (pct < 100) requestAnimationFrame(tick);
      else setTimeout(() => { setScan(null); setScanTime(+(1.3 + Math.random() * 0.3).toFixed(2)); toast.success("Scan complete", { description: `${TOTAL_FILES.toLocaleString()} files indexed` }); }, 150);
    };
    requestAnimationFrame(tick);
  };

  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); setCmdOpen((o) => !o); } };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  const copyPath = (n: FsNode) => { navigator.clipboard?.writeText(n.path).catch(() => {}); toast("Path copied", { description: n.path }); };
  const largestFiles = useMemo(() => all.filter((n) => !n.isDir || n.junkTag).sort((a, b) => b.sizeBytes - a.sizeBytes).slice(0, 12), [all]);
  const used = tree.sizeBytes;

  return (
    <TooltipProvider delayDuration={200}>
      <div className="flex h-dvh flex-col overflow-hidden bg-background text-foreground">
        {/* Header */}
        <header className="relative flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-card px-3 py-2 md:gap-3 md:px-4 lg:h-14 lg:flex-nowrap lg:py-0">
          <Logo />
          <span className="text-sm font-semibold tracking-tight">DiskSieve</span>
          <Badge variant="outline" className="hidden font-mono text-[10px] text-muted-foreground sm:inline-flex">v0.1.0</Badge>
          <Select defaultValue="c" onValueChange={(v) => v !== "c" && toast(v === "d" ? "D:\\ Archive is offline in this prototype" : "Folder picker opens in the desktop build")}>
            <SelectTrigger className="h-8 w-auto min-w-0 max-w-[60vw] text-xs sm:ml-2 sm:w-[270px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="c">
                <span className="flex items-center gap-2"><HardDrive className="h-3.5 w-3.5 text-primary" />C:\ Local Disk <span className="font-mono text-muted-foreground">({formatBytes(used)} / 1.0 TB)</span>
                  <span className="h-1 w-10 overflow-hidden rounded-full bg-muted"><span className="block h-full bg-primary" style={{ width: `${(used / DRIVE_CAPACITY) * 100}%` }} /></span></span>
              </SelectItem>
              <SelectItem value="d"><span className="flex items-center gap-2"><HardDrive className="h-3.5 w-3.5" />D:\ Archive <span className="font-mono text-muted-foreground">(1.2 TB / 2.0 TB)</span></span></SelectItem>
              <SelectItem value="folder"><span className="flex items-center gap-2"><FolderOpen className="h-3.5 w-3.5" />Select Folder...</span></SelectItem>
            </SelectContent>
          </Select>
          <Button size="sm" variant="outline" className="h-8" onClick={rescan} disabled={!!scan} aria-label="Rescan"><RefreshCw className={cn("h-3.5 w-3.5", scan && "animate-spin")} /><span className="hidden sm:inline">Rescan</span></Button>

          <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)} className="order-last w-full lg:order-none lg:mx-auto lg:w-auto">
            <TabsList className="h-9 w-full lg:w-auto">
              <TabsTrigger value="explorer" className="flex-1 text-xs">Explorer</TabsTrigger>
              <TabsTrigger value="junk" className="flex-1 gap-1.5 text-xs">Dev Junk <Badge className="h-4 px-1.5 font-mono text-[10px]">{formatBytes(junkTotal)}</Badge></TabsTrigger>
              <TabsTrigger value="dups" className="flex-1 gap-1.5 text-xs">Duplicates <Badge variant="secondary" className="h-4 px-1.5 font-mono text-[10px]">{formatBytes(wastedBytes(dups))}</Badge></TabsTrigger>
            </TabsList>
          </Tabs>

          <div className="ml-auto flex items-center gap-2 md:gap-3 lg:ml-0">
            <label className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground">
              <Switch checked={delta} onCheckedChange={setDelta} />
              <span className={cn(delta && "text-primary")}><span className="hidden xl:inline">Delta Mode (vs. Yesterday)</span><span className="xl:hidden">Delta</span></span>
            </label>
            <Button size="sm" variant="outline" aria-label="Search files" className="h-8 justify-between text-xs text-muted-foreground md:w-48" onClick={() => setCmdOpen(true)}>
              <span className="flex items-center gap-2"><Search className="h-3.5 w-3.5" /><span className="hidden md:inline">Search files...</span></span>
              <kbd className="hidden md:inline rounded border border-border px-1 font-mono text-[10px]">⌘K</kbd>
            </Button>
          </div>
          {scan && (
            <div className="absolute inset-x-0 -bottom-px">
              <Progress value={scan.pct} className="h-0.5 rounded-none" />
            </div>
          )}
        </header>

        {tab === "explorer" && (
          <>
            {/* Sub-toolbar */}
            <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-3 py-2 md:gap-3 md:px-4 lg:h-14 lg:flex-nowrap lg:py-0">
              <Button size="icon" variant="ghost" className="h-8 w-8" disabled={trail.length < 2} onClick={() => setCurrentId(trail[trail.length - 2]!.id)} aria-label="Up one level"><ArrowUp className="h-4 w-4" /></Button>
              <Breadcrumb className="min-w-0 flex-1 overflow-x-auto">
                <BreadcrumbList className="flex-nowrap whitespace-nowrap text-xs">
                  {trail.map((n, i) => (
                    <span key={n.id} className="contents">
                      {i > 0 && <BreadcrumbSeparator />}
                      <BreadcrumbItem>
                        {i === trail.length - 1 ? <BreadcrumbPage className="font-mono">{n.name}</BreadcrumbPage>
                          : <BreadcrumbLink className="cursor-pointer font-mono" onClick={() => setCurrentId(n.id)}>{n.name}</BreadcrumbLink>}
                      </BreadcrumbItem>
                    </span>
                  ))}
                </BreadcrumbList>
              </Breadcrumb>
              <div className="flex max-w-full overflow-x-auto rounded-md border border-border bg-card p-0.5">
                {MODES.map((m) => (
                  <button key={m.key} onClick={() => setMode(m.key)} className={cn("whitespace-nowrap rounded px-2 py-1 text-xs transition-colors sm:px-3", mode === m.key ? "bg-primary/15 text-primary" : "text-muted-foreground hover:text-foreground")}>{m.label}</button>
                ))}
              </div>
              <div className="hidden flex-1 items-center justify-end gap-2 md:flex">
                <span className="rounded-full border border-border bg-card px-3 py-1 font-mono text-[11px] text-muted-foreground">
                  {scan ? `${scan.files.toLocaleString()} files • scanning…` : `${TOTAL_FILES.toLocaleString()} files • Scanned in ${scanTime}s`}
                </span>
                <Tooltip><TooltipTrigger asChild>
                  <Badge variant="outline" className="border-warning/40 text-[10px] text-warning">3 system folders skipped (Access Denied)</Badge>
                </TooltipTrigger><TooltipContent className="font-mono text-xs">System Volume Information · $Recycle.Bin · Config.Msi</TooltipContent></Tooltip>
              </div>
            </div>

            {/* Workspace */}
            <div className="flex min-h-0 flex-1 flex-col overflow-y-auto lg:flex-row lg:overflow-hidden">
              <div className="flex h-[55vh] shrink-0 flex-col border-b border-border p-3 lg:h-auto lg:w-[68%] lg:border-b-0 lg:border-r">
                <ContextMenu>
                  <ContextMenuTrigger asChild>
                    <div className="min-h-0 flex-1 overflow-hidden rounded-md">
                      {items.length ? <Treemap parent={current} items={items} mode={mode} delta={delta} onActivate={activate} onContextTarget={setCtxNode} />
                        : <div className="flex h-full items-center justify-center text-sm text-muted-foreground">{delta ? "No growth since yesterday in this folder." : "This folder is empty."}</div>}
                    </div>
                  </ContextMenuTrigger>
                  <ContextMenuContent className="w-60">
                    <div className="truncate px-2 py-1.5 font-mono text-[11px] text-muted-foreground">{ctxNode?.name ?? "No item under cursor"}</div>
                    <ContextMenuSeparator />
                    <ContextMenuItem disabled={!ctxNode} onSelect={() => ctxNode && setInspectId(ctxNode.id)}><Eye className="h-4 w-4" />Inspect & Safety Advisor</ContextMenuItem>
                    <ContextMenuItem disabled={!ctxNode} onSelect={() => toast("Opening in File Explorer", { description: ctxNode?.path })}><ExternalLink className="h-4 w-4" />Open in File Explorer</ContextMenuItem>
                    <ContextMenuItem disabled={!ctxNode} onSelect={() => ctxNode && copyPath(ctxNode)}><Copy className="h-4 w-4" />Copy Full Path</ContextMenuItem>
                    <ContextMenuSeparator />
                    <ContextMenuItem disabled={!ctxNode} className="text-destructive focus:text-destructive" onSelect={() => ctxNode && trash([ctxNode])}><Trash2 className="h-4 w-4" />Move to Trash</ContextMenuItem>
                  </ContextMenuContent>
                </ContextMenu>
                <div className="mt-2 flex flex-wrap items-center gap-4 text-[11px] text-muted-foreground">
                  {legendFor(mode).map((l) => (
                    <span key={l.label} className="flex items-center gap-1.5"><span className="h-2.5 w-2.5 rounded-sm" style={{ background: l.color }} />{l.label}</span>
                  ))}
                  <span className="ml-auto hidden font-mono sm:inline">{delta ? "showing growth since yesterday" : "right-click a tile for actions"}</span>
                </div>
              </div>

              <div className="flex min-h-[50vh] flex-col lg:min-h-0 lg:w-[32%]">
                <div className="flex items-center justify-between border-b border-border px-4 py-2.5 text-xs text-muted-foreground">
                  <span>{items.length} items · sorted by {delta ? "growth" : "size"}</span>
                  <span className="font-mono text-foreground">{formatBytes(delta ? current.deltaBytes : current.sizeBytes, delta)}</span>
                </div>
                <div className="min-h-0 flex-1 overflow-auto">
                  {items.map((n) => {
                    const pct = (Math.max(val(n), 0) / folderTotal) * 100;
                    return (
                      <button key={n.id} onClick={() => activate(n)} onContextMenu={(e) => { e.preventDefault(); setInspectId(n.id); }}
                        className="group relative flex w-full items-center gap-2.5 border-b border-border/60 px-4 py-2 text-left text-sm hover:bg-accent/40">
                        <span className="absolute inset-y-0 left-0 bg-primary/10" style={{ width: `${pct}%` }} />
                        {n.isDir ? <Folder className="relative h-4 w-4 shrink-0 text-primary" /> : <File className="relative h-4 w-4 shrink-0 text-muted-foreground" />}
                        <span className="relative min-w-0 flex-1 truncate">{n.name}</span>
                        {n.junkTag && <Badge variant="outline" className="relative h-4 border-primary/40 px-1.5 font-mono text-[9px] text-primary">{n.junkTag}</Badge>}
                        <span className="relative hidden w-14 shrink-0 text-right font-mono text-[11px] sm:inline text-muted-foreground">{formatAge(n.lastModifiedDaysAgo)}</span>
                        <span className={cn("relative w-20 shrink-0 text-right font-mono text-xs", delta && "text-primary")}>{formatBytes(val(n), delta)}</span>
                        {n.isDir && n.children?.length ? <ChevronRight className="relative h-3.5 w-3.5 text-muted-foreground opacity-0 group-hover:opacity-100" /> : <span className="w-3.5" />}
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          </>
        )}
        {tab === "junk" && <DevJunkView rows={junkRows} onTrash={trash} />}
        {tab === "dups" && <DuplicatesView groups={dups} setGroups={setDups} />}

        {/* Inspector */}
        <Sheet open={!!inspected} onOpenChange={(o) => !o && setInspectId(null)}>
          <SheetContent className="w-full overflow-y-auto sm:w-[420px] sm:max-w-[420px]">
            {inspected && <Inspector n={inspected} onTrash={() => trash([inspected])} onCopy={() => copyPath(inspected)} />}
          </SheetContent>
        </Sheet>

        {/* Command palette */}
        <CommandDialog open={cmdOpen} onOpenChange={setCmdOpen}>
          <CommandInput placeholder="Search files, folders, or actions…" />
          <CommandList>
            <CommandEmpty>No results.</CommandEmpty>
            <CommandGroup heading="Largest items on drive">
              {largestFiles.map((n) => (
                <CommandItem key={n.id} value={`${n.name} ${n.path}`} onSelect={() => { const p = findPath(tree, n.id); if (p && p.length > 1) setCurrentId(p[p.length - 2]!.id); setTab("explorer"); setInspectId(n.id); setCmdOpen(false); }}>
                  {n.isDir ? <Folder className="text-primary" /> : <File />}
                  <span className="truncate">{n.name}</span>
                  <span className="ml-2 truncate font-mono text-[10px] text-muted-foreground">{n.path}</span>
                  <CommandShortcut className="font-mono">{formatBytes(n.sizeBytes)}</CommandShortcut>
                </CommandItem>
              ))}
            </CommandGroup>
            <CommandGroup heading="View">
              {MODES.map((m) => <CommandItem key={m.key} onSelect={() => { setMode(m.key); setTab("explorer"); setCmdOpen(false); }}>Color by: {m.label}</CommandItem>)}
              <CommandItem onSelect={() => { setDelta((d) => !d); setCmdOpen(false); }}>Toggle Delta Mode</CommandItem>
              <CommandItem onSelect={() => { setTab("junk"); setCmdOpen(false); }}>Open Dev Junk cleaner</CommandItem>
              <CommandItem onSelect={() => { setTab("dups"); setCmdOpen(false); }}>Open Duplicates finder</CommandItem>
              <CommandItem onSelect={() => { setCmdOpen(false); rescan(); }}>Rescan drive</CommandItem>
            </CommandGroup>
          </CommandList>
        </CommandDialog>
        <Toaster position="bottom-right" theme="dark" />
      </div>
    </TooltipProvider>
  );
}

function Inspector({ n, onTrash, onCopy }: { n: FsNode; onTrash: () => void; onCopy: () => void }) {
  const system = n.safetyHint.startsWith("SYSTEM");
  const safe = n.safetyHint.startsWith("Safe");
  const rows: [string, string][] = [
    ["Path", n.path], ["Exact Bytes", n.sizeBytes.toLocaleString() + " B"], ["IEC Size", formatBytes(n.sizeBytes).replace(" GB", " GiB").replace(" MB", " MiB")],
    ["Growth (24h)", formatBytes(n.deltaBytes, true)], ["File Count", n.fileCount.toLocaleString()], ["Last Modified", formatAge(n.lastModifiedDaysAgo)],
    ["Extension", n.isDir ? "— (folder)" : n.extension ? `.${n.extension}` : "—"],
  ];
  return (
    <div className="flex h-full flex-col gap-5">
      <SheetHeader>
        <SheetTitle className="flex items-center gap-2">{n.isDir ? <Folder className="h-5 w-5 text-primary" /> : <File className="h-5 w-5" />}{n.name}</SheetTitle>
        <SheetDescription className="font-mono text-2xl text-foreground">{formatBytes(n.sizeBytes)}</SheetDescription>
      </SheetHeader>
      <dl className="grid grid-cols-[110px_1fr] gap-x-3 gap-y-2 rounded-lg border border-border bg-background p-4 text-xs">
        {rows.map(([k, v]) => (<Fragment key={k}><dt className="text-muted-foreground">{k}</dt><dd className="break-all font-mono">{v}</dd></Fragment>))}
      </dl>
      <div className={cn("rounded-lg border p-4", safe ? "border-primary/40 bg-primary/10" : system ? "border-destructive/40 bg-destructive/10" : "border-warning/40 bg-warning/10")}>
        <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
          {safe ? <ShieldCheck className="h-4 w-4 text-primary" /> : <ShieldAlert className={cn("h-4 w-4", system ? "text-destructive" : "text-warning")} />}
          Can I delete this?
          <Badge className={cn("ml-auto", safe ? "" : system ? "bg-destructive text-destructive-foreground" : "bg-warning text-warning-foreground")}>{safe ? "Safe to delete" : system ? "Do not delete" : "Review first"}</Badge>
        </div>
        <p className="text-sm text-muted-foreground">{n.safetyHint.replace(/^SYSTEM: /, "")}</p>
      </div>
      <div className="mt-auto flex gap-2">
        <Button variant="outline" className="flex-1" onClick={onCopy}><Copy className="h-4 w-4" />Copy Path</Button>
        <Button variant="destructive" className="flex-1" disabled={system} onClick={onTrash}><Trash2 className="h-4 w-4" />Move to Trash</Button>
      </div>
    </div>
  );
}
