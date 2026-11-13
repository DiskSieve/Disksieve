// Tree-shaken Apache ECharts build: registers only what the treemap needs.
// Importing the full `echarts` package pulls ~1.1 MB of unused charts into the bundle.
import * as echarts from "echarts/core";
import { TreemapChart } from "echarts/charts";
import { TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";

echarts.use([TreemapChart, TooltipComponent, CanvasRenderer]);

export { echarts };
export type { ECharts } from "echarts/core";
