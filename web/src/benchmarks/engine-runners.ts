export interface BenchmarkEngineRunner {
  id: "mmdflux" | "mermaid";
  label: string;
  warm: (input: string) => Promise<void>;
  render: (input: string) => Promise<string>;
}

interface MmdfluxWasmModule {
  default: () => Promise<void>;
  render: (input: string, format: string, configJson: string) => string;
}

interface MermaidRenderResult {
  svg: string;
}

interface MermaidApi {
  initialize?: (config: Record<string, unknown>) => void;
  render:
    | ((id: string, input: string) => Promise<string | MermaidRenderResult>)
    | ((id: string, input: string) => string | MermaidRenderResult);
}

interface MermaidModule {
  default?: MermaidApi;
  initialize?: MermaidApi["initialize"];
  render?: MermaidApi["render"];
}

export interface CreateBenchmarkEngineRunnersOptions {
  loadMmdfluxModule?: () => Promise<MmdfluxWasmModule>;
  loadMermaidModule?: () => Promise<MermaidModule>;
}

export async function loadMmdfluxModule(): Promise<MmdfluxWasmModule> {
  const { loadWasmModule } = await import("../wasm-module");
  return (await loadWasmModule()) as MmdfluxWasmModule;
}

export async function loadMermaidModule(): Promise<MermaidModule> {
  return (await import("mermaid")) as MermaidModule;
}

function wrapRunner(
  id: BenchmarkEngineRunner["id"],
  label: string,
  render: (input: string) => Promise<string>,
): BenchmarkEngineRunner {
  return {
    id,
    label,
    warm: async (input) => {
      await render(input);
    },
    render,
  };
}

function toMermaidApi(module: MermaidModule): MermaidApi {
  if (module.default) {
    return module.default;
  }
  if (typeof module.render === "function") {
    return {
      initialize: module.initialize,
      render: module.render,
    };
  }

  throw new Error("mermaid module does not expose a render() API");
}

function normalizeMermaidSvg(result: string | MermaidRenderResult): string {
  if (typeof result === "string") {
    return result;
  }
  if (typeof result.svg === "string") {
    return result.svg;
  }

  throw new Error("mermaid runner returned an invalid render result");
}

export async function createBenchmarkEngineRunners(
  options: CreateBenchmarkEngineRunnersOptions = {},
): Promise<[BenchmarkEngineRunner, BenchmarkEngineRunner]> {
  const loadMmdflux = options.loadMmdfluxModule ?? loadMmdfluxModule;
  const loadMermaid = options.loadMermaidModule ?? loadMermaidModule;

  const [mmdfluxModule, mermaidModule] = await Promise.all([
    loadMmdflux(),
    loadMermaid(),
  ]);

  await mmdfluxModule.default();

  const mmdfluxRunner = wrapRunner("mmdflux", "mmdflux (Wasm)", async (input) =>
    mmdfluxModule.render(input, "svg", "{}"),
  );

  const mermaidApi = toMermaidApi(mermaidModule);
  mermaidApi.initialize?.({
    startOnLoad: false,
    securityLevel: "strict",
  });
  let sequence = 0;
  const mermaidRunner = wrapRunner("mermaid", "mermaid.js", async (input) => {
    const renderOutput = await mermaidApi.render(
      `mmdflux-benchmark-${sequence++}`,
      input,
    );
    return normalizeMermaidSvg(renderOutput);
  });

  return [mmdfluxRunner, mermaidRunner];
}
