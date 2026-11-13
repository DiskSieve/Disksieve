import { createFileRoute } from "@tanstack/react-router";
import { DiskSieveApp } from "@/components/disksieve/DiskSieveApp";

export const Route = createFileRoute("/")({
  head: () => ({
    meta: [
      { title: "DiskSieve — Disk Space Analyzer & Storage Cleaner" },
      { name: "description", content: "Visualize what fills your drive, find dev junk and duplicates, and reclaim space safely." },
      { property: "og:title", content: "DiskSieve — Disk Space Analyzer" },
      { property: "og:description", content: "Treemap explorer, dev-junk cleaner and duplicate finder for your drive." },
      { property: "og:type", content: "website" },
      { name: "twitter:card", content: "summary_large_image" },
    ],
  }),
  component: DiskSieveApp,
});
