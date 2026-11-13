import { useState } from "react";
import { Copy, Trash2, Wand2 } from "lucide-react";
import { toast } from "sonner";
import { duplicateGroups, formatAge, formatBytes, type DupGroup } from "@/data/mockDrive";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";

export const wastedBytes = (g: DupGroup[]) => g.reduce((a, x) => a + x.sizeBytes * (x.copies.length - 1), 0);

export function DuplicatesView({ groups, setGroups }: { groups: DupGroup[]; setGroups: (g: DupGroup[]) => void }) {
  const [sel, setSel] = useState<Set<string>>(new Set());
  const key = (g: DupGroup, p: string) => `${g.id}|${p}`;
  const smart = () => {
    const s = new Set<string>();
    groups.forEach((g) => { const oldest = g.copies.reduce((a, c) => (c.daysAgo > a.daysAgo ? c : a)); g.copies.forEach((c) => c !== oldest && s.add(key(g, c.path))); });
    setSel(s);
  };
  const selBytes = groups.reduce((a, g) => a + g.copies.filter((c) => sel.has(key(g, c.path))).length * g.sizeBytes, 0);
  const trash = () => {
    if (groups.some((g) => g.copies.every((c) => sel.has(key(g, c.path))))) { toast.error("Keep at least one copy in every group"); return; }
    const prev = groups;
    setGroups(groups.map((g) => ({ ...g, copies: g.copies.filter((c) => !sel.has(key(g, c.path))) })).filter((g) => g.copies.length > 1));
    setSel(new Set());
    toast.success(`Moved ${formatBytes(selBytes)} of duplicates to Trash`, { action: { label: "Undo", onClick: () => setGroups(prev) } });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-3 md:gap-4 md:p-5">
      <div className="flex flex-wrap items-center justify-between gap-4 rounded-lg border border-border bg-card p-4 md:p-5">
        <div className="text-xl font-semibold md:text-2xl"><span className="font-mono text-primary">{formatBytes(wastedBytes(groups))}</span> <span className="text-base font-normal text-muted-foreground">wasted across {groups.length} duplicate file groups</span></div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={smart}><Wand2 className="h-4 w-4" />Smart Select (Keep Oldest)</Button>
          <Button disabled={!sel.size} onClick={trash} className="shadow-glow"><Trash2 className="h-4 w-4" />Trash Selected Duplicates <span className="font-mono">({formatBytes(selBytes)})</span></Button>
        </div>
      </div>
      <div className="min-h-0 flex-1 space-y-3 overflow-auto pr-1">
        {groups.length === 0 && <div className="py-20 text-center text-muted-foreground">No duplicates left. Nice and tidy.</div>}
        {groups.map((g) => (
          <div key={g.id} className="rounded-lg border border-border bg-card">
            <div className="flex items-center gap-3 border-b border-border px-4 py-3">
              <Copy className="h-4 w-4 text-primary" />
              <span className="font-medium">{g.name}</span>
              <Badge variant="outline" className="font-mono text-[11px] text-muted-foreground">{g.hash}</Badge>
              <span className="ml-auto font-mono text-sm">{formatBytes(g.sizeBytes)} <span className="text-muted-foreground">× {g.copies.length}</span></span>
            </div>
            {g.copies.map((c) => (
              <label key={c.path} className="flex cursor-pointer items-center gap-3 px-4 py-2 text-sm hover:bg-accent/40">
                <Checkbox checked={sel.has(key(g, c.path))} onCheckedChange={() => setSel((s) => { const n = new Set(s); const k = key(g, c.path); n.has(k) ? n.delete(k) : n.add(k); return n; })} />
                <span className="truncate font-mono text-xs text-muted-foreground">{c.path}</span>
                <span className="ml-auto shrink-0 font-mono text-xs text-muted-foreground">{formatAge(c.daysAgo)}</span>
              </label>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
export const initialDuplicates = duplicateGroups;
