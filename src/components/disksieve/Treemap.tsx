import { useEffect, useRef } from "react";
import type { FsNode } from "@/data/mockDrive";
import { formatAge, formatBytes } from "@/data/mockDrive";
import { colorFor, type ColorMode } from "@/lib/treemap-palette";

type ECharts = import("echarts").ECharts;

interface Props {
  parent: FsNode;
  items: FsNode[];
  mode: ColorMode;
  delta: boolean;
  onActivate: (n: FsNode) => void;
  onContextTarget: (n: FsNode | null) => void;
}

export function Treemap({ parent, items, mode, delta, onActivate, onContextTarget }: Props) {
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<ECharts | null>(null);
  const handlers = useRef({ onActivate, onContextTarget, items });
  handlers.current = { onActivate, onContextTarget, items };

  useEffect(() => {
    let disposed = false;
    let ro: ResizeObserver | undefined;
    import("echarts").then((ec) => {
      if (disposed || !el.current) return;
      const c = ec.init(el.current, undefined, { renderer: "canvas" });
      chart.current = c;
      const byId = (id: string) => handlers.current.items.find((n) => n.id === id);
      c.on("click", (p) => { const n = byId((p.data as { id: string })?.id); if (n) handlers.current.onActivate(n); });
      c.on("contextmenu", (p) => { handlers.current.onContextTarget(byId((p.data as { id: string })?.id) ?? null); });
      ro = new ResizeObserver(() => c.resize());
      ro.observe(el.current);
      render();
    });
    return () => { disposed = true; ro?.disconnect(); chart.current?.dispose(); chart.current = null; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const render = () => {
    const c = chart.current;
    if (!c) return;
    const val = (n: FsNode) => (delta ? Math.max(n.deltaBytes, 0) : n.sizeBytes);
    const total = items.reduce((a, n) => a + val(n), 0) || 1;
    const data = items.filter((n) => val(n) > 0).map((n) => ({
      id: n.id, name: n.name, value: val(n), node: n,
      itemStyle: { color: colorFor(n, mode), borderColor: "#0B0D0C" },
    }));
    if (!delta && items.length > 2) {
      data.push({ id: "__other", name: "Other (1,420 small files)", value: total * 0.012, node: undefined as unknown as FsNode, itemStyle: { color: "#1A1F1D", borderColor: "#0B0D0C" } });
    }
    c.setOption({
      animationDurationUpdate: 450,
      tooltip: {
        backgroundColor: "#121514", borderColor: "#1F2422", padding: [8, 10],
        textStyle: { color: "#E8EDEA", fontFamily: "Geist, sans-serif", fontSize: 12 },
        formatter: (p: { data: { node?: FsNode; name: string; value: number } }) => {
          const n = p.data.node;
          const pct = ((p.data.value / total) * 100).toFixed(1);
          const mono = "font-family:JetBrains Mono,monospace";
          if (!n) return `<b>${p.data.name}</b><br/><span style="${mono};color:#8A968F">${formatBytes(p.data.value)} · aggregated</span>`;
          return `<div style="min-width:180px"><b>${n.name}</b>${n.junkTag ? ` <span style="color:#1ED760">· ${n.junkTag}</span>` : ""}<br/>
            <span style="${mono}">${formatBytes(p.data.value, delta)}</span> <span style="color:#8A968F">· ${pct}% of ${parent.name}</span><br/>
            <span style="color:#8A968F;${mono};font-size:11px">${n.fileCount.toLocaleString()} files · ${formatAge(n.lastModifiedDaysAgo)}</span></div>`;
        },
      },
      series: [{
        type: "treemap", data, roam: false, nodeClick: false, width: "100%", height: "100%", top: 0, left: 0, right: 0, bottom: 0,
        breadcrumb: { show: false }, squareRatio: 0.9,
        label: {
          show: true, position: "insideTopLeft", padding: 8, overflow: "truncate",
          formatter: (p: { name: string; value: number }) => `{n|${p.name}}\n{s|${formatBytes(p.value, delta)}}`,
          rich: {
            n: { color: "#F2F5F3", fontSize: 12, fontWeight: 600, fontFamily: "Geist, sans-serif", lineHeight: 16 },
            s: { color: "rgba(242,245,243,0.7)", fontSize: 11, fontFamily: "JetBrains Mono, monospace" },
          },
        },
        itemStyle: { borderWidth: 2, gapWidth: 2, borderRadius: 4 },
        emphasis: { itemStyle: { borderColor: "#22C55E", borderWidth: 2 } },
      }],
    }, { replaceMerge: ["series"] });
  };

  useEffect(render);

  return (
    <div className="h-full w-full" onContextMenuCapture={() => onContextTarget(null)}>
      <div ref={el} className="h-full w-full" />
    </div>
  );
}
