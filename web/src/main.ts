import "../styles/main.css";

export {
  bootstrapPlaygroundApp,
  isBenchmarkModeEnabled,
  type RenderAppOptions,
  renderApp,
} from "./app/bootstrap";
export type { RenderWorkerClient } from "./services/render-client";

import { bootstrapPlaygroundApp } from "./app/bootstrap";

const root = document.querySelector<HTMLElement>("#app");
if (root) {
  void bootstrapPlaygroundApp(root);
}
