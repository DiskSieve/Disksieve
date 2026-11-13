import { useMemo, useState } from "react";
import { Trash2, Sparkles } from "lucide-react";
import type { FsNode } from "@/data/mockDrive";
import { formatAge, formatBytes } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { cn } from "@/lib/utils";

export interface JunkRow { node: FsNode; project: string }
const TAG_LABEL: Record<string, string> = { node_modules: "node_modules", cargo_target: "target/", python_venv: ".venv", gradle: ".gradle", docker_cache: "docker cache" };
const FILTERS = [
  { key: "all", label: "All Artifacts" }, { key: "cold", label: "Untouched > 60 days" },
  { key: "node_modules", label: "node_modules" }, { key: "cargo_target", label: "Rust target/" }, { key: "python_venv", label: "Python .venv" },
];

export function DevJunkView({ rows, onTrash }: { rows: JunkRow[]; onTrash: (nodes: FsNode[]) => void }) {
  const [filter, setFilter] = useState("all");
  const [selected, setSelected] = useState<Set<string>>(() => new Set(rows.filter((r) => r.node.lastModifiedDaysAgo > 60).map((r) => r.node.id)));
  const visible = useMemo(() => rows.filter((r) => filter === "all" ? true : filter === "cold" ? r.node.lastModifiedDaysAgo > 60 : r.node.junkTag === filter), [rows, filter]);
  const total = rows.reduce((a, r) => a + r.node.sizeBytes, 0);
  const sel = rows.filter((r) => selected.has(r.node.id));
  const selBytes = sel.reduce((a, r) => a + r.node.sizeBytes, 0);
  const toggle = (id: string) => setSelected((s) => { const n = new Set(s); n.has(id) ? n.delete(id) : n.add(id); return n; });
  const allVisible = visible.length > 0 && visible.every((r) => selected.has(r.node.id));

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-3 md:gap-4 md:p-5">
      <div className="flex flex-wrap items-center justify-between gap-4 rounded-lg border border-border bg-card p-4 md:p-5">
        <div>
          <div className="flex items-center gap-2 text-xs uppercase tracking-wider text-muted-foreground"><Sparkles className="h-3.5 w-3.5 text-primary" />Reclaimable developer artifacts</div>
          <div className="mt-1 text-xl font-semibold md:text-2xl"><span className="font-mono text-primary">{formatBytes(total)}</span> <span className="text-base font-normal text-muted-foreground">across {new Set(rows.map((r) => r.project)).size} developer projects</span></div>
          <div className="mt-3 flex flex-wrap gap-1.5">
            {FILTERS.map((f) => (
              <button key={f.key} onClick={() => setFilter(f.key)} className={cn("rounded-full border px-3 py-1 text-xs transition-colors", filter === f.key ? "border-primary/60 bg-primary/15 text-primary" : "border-border text-muted-foreground hover:text-foreground")}>{f.label}</button>
            ))}
          </div>
        </div>
        <Button disabled={!sel.length} onClick={() => { onTrash(sel.map((r) => r.node)); setSelected(new Set()); }} className="shadow-glow">
          <Trash2 className="h-4 w-4" />Move Selected to Trash <span className="font-mono">({formatBytes(selBytes)})</span>
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto rounded-lg border border-border bg-card">
        <Table>
          <TableHeader className="sticky top-0 bg-card">
            <TableRow>
              <TableHead className="w-10"><Checkbox checked={allVisible} onCheckedChange={(v) => setSelected((s) => { const n = new Set(s); visible.forEach((r) => v ? n.add(r.node.id) : n.delete(r.node.id)); return n; })} /></TableHead>
              <TableHead>Project</TableHead><TableHead>Artifact</TableHead><TableHead className="hidden md:table-cell">Path</TableHead><TableHead>Last Code Edit</TableHead><TableHead className="text-right">Size</TableHead><TableHead className="w-12" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {visible.map((r) => (
              <TableRow key={r.node.id} data-state={selected.has(r.node.id) ? "selected" : undefined}>
                <TableCell><Checkbox checked={selected.has(r.node.id)} onCheckedChange={() => toggle(r.node.id)} /></TableCell>
                <TableCell className="font-medium">{r.project}</TableCell>
                <TableCell><Badge variant="outline" className="border-primary/40 font-mono text-primary">{TAG_LABEL[r.node.junkTag ?? ""]}</Badge></TableCell>
                <TableCell className="hidden max-w-[340px] truncate md:table-cell font-mono text-xs text-muted-foreground">{r.node.path}</TableCell>
                <TableCell className={cn("font-mono text-xs", r.node.lastModifiedDaysAgo > 60 ? "text-warning" : "text-muted-foreground")}>{formatAge(r.node.lastModifiedDaysAgo)}</TableCell>
                <TableCell className="text-right font-mono">{formatBytes(r.node.sizeBytes)}</TableCell>
                <TableCell><Button size="icon" variant="ghost" className="h-7 w-7 text-muted-foreground hover:text-destructive" onClick={() => onTrash([r.node])} aria-label="Trash"><Trash2 className="h-3.5 w-3.5" /></Button></TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
