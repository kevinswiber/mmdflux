# Releasing

This project publishes:

- Crate releases to crates.io
- Binary release assets to GitHub Releases
- `@mmds/wasm` npm package
- `@mmds/core`, `@mmds/excalidraw`, `@mmds/tldraw` npm packages
- Homebrew formula updates in `kevinswiber/homebrew-mmdflux`

## Release Checklist

1. Ensure `main` is green in CI.
2. Rename the `## Unreleased` section in `CHANGELOG.md` to `## vX.Y.Z` (keep an empty `## Unreleased` above it).
3. Bump version in `Cargo.toml`, `crates/mmdflux-wasm/Cargo.toml`, and `Cargo.lock`.
4. Commit and push the version bump and changelog.
5. Tag and push:

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

6. Confirm the three release workflows complete:
   - **Release** — builds binaries and publishes GitHub Release assets
   - **Crate Release** — publishes `mmdflux` to crates.io
   - **WASM Release** — publishes `@mmds/wasm` to npm

## GitHub Release Assets

The release workflow publishes these artifacts:

- `mmdflux-vX.Y.Z-darwin-arm64.tar.gz`
- `mmdflux-vX.Y.Z-darwin-x86_64.tar.gz`
- `mmdflux-vX.Y.Z-linux-x86_64.tar.gz`
- `mmdflux-vX.Y.Z-windows-x86_64.zip`
- `checksums.txt`

## npm Adapter Packages

The adapter packages (`@mmds/core`, `@mmds/excalidraw`, `@mmds/tldraw`) version independently from the main crate. They are managed as cocogitto monorepo packages.

### Bumping a Package

Use `cog bump --package` to bump an individual package:

```bash
# Automatic version based on conventional commits since last tag
cog bump --package mmds-core --auto

# Or specify the bump level
cog bump --package excalidraw --minor
cog bump --package tldraw --patch
```

This will:
1. Compute the next version from commit history (or use the specified level)
2. Run `npm version` to update `package.json`
3. Generate a package-specific changelog
4. Commit the changes
5. Create a tag (e.g., `mmds-core-0.2.0`)
6. Push the commit and tag

The tag push triggers the **Packages Release** workflow, which builds and publishes the package to npm.

### Coordinating @mmds/core Updates

When `@mmds/core` has a breaking change that affects the adapters:

1. Bump `@mmds/core` first: `cog bump --package mmds-core --minor`
2. Update the `@mmds/core` dependency version in `excalidraw` and `tldraw` package.json files
3. Run `npm install` in `packages/` to update the lockfile
4. Bump each adapter: `cog bump --package excalidraw --minor`, etc.

For compatible (patch) changes, the caret range (`^0.x.y`) handles resolution automatically — no adapter bump is needed.

### Trusted Publishing Setup

Each package must be configured on [npmjs.com](https://www.npmjs.com/) to trust the GitHub Actions workflow:

- **Repository:** `kevinswiber/mmdflux` (or the org/repo)
- **Workflow:** `packages-release.yml`
- **Environment:** (default, no named environment)

This must be done once per package in the npm package settings under "Publishing access".

## Homebrew Tap

Tap repository:

- [kevinswiber/homebrew-mmdflux](https://github.com/kevinswiber/homebrew-mmdflux)

Install command for users:

```bash
brew tap kevinswiber/mmdflux
brew install mmdflux
```

### Updating Homebrew Formula for a New Release

1. Clone/update tap repo:

```bash
git clone git@github.com:kevinswiber/homebrew-mmdflux.git
cd homebrew-mmdflux
```

2. Pull release checksums:

```bash
gh release download vX.Y.Z --repo kevinswiber/mmdflux --pattern checksums.txt
cat checksums.txt
```

3. Update `Formula/mmdflux.rb` with new version, URLs, and SHA256 values.
4. Commit and push the formula update.
