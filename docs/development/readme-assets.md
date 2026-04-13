# README Assets

The main README references pre-rendered showcase assets (SVG, text, MMDS JSON) in `docs/assets/readme/`. These are checked into the repo and should be refreshed whenever rendering output changes significantly.

## Refreshing assets

```bash
./scripts/refresh-readme-assets.sh
```

This regenerates all showcase variants from the source diagram at `docs/assets/readme/at-a-glance.mmd`:

| Output | Description |
|--------|-------------|
| `at-a-glance.txt` | Unicode text rendering |
| `at-a-glance.svg` | SVG (light background, historical default path) |
| `at-a-glance-light.svg` | SVG with `#ffffff` background |
| `at-a-glance-dark.svg` | SVG tuned for GitHub dark themes (`#0d1117` background) |
| `at-a-glance.mmds.json` | MMDS JSON with routed geometry |

The script uses `flux-layered` engine, `smooth-step` edge preset, and `routed` geometry level.

## Options

```
-s, --source <path>       Mermaid input file (default: docs/assets/readme/at-a-glance.mmd)
-o, --out-dir <path>      Output directory (default: docs/assets/readme)
-n, --name <name>         Output basename (default: at-a-glance)
    --mmdflux-bin <path>  Use a prebuilt binary instead of cargo run
```

You can also set `MMDFLUX_BIN` as an environment variable.
