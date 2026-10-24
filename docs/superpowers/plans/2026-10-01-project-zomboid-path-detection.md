# Project Zomboid Path Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the starter Tauri screen with a Windows dashboard that detects and displays Project Zomboid paths plus local Steam account information at startup.

**Architecture:** Rust owns Steam discovery and exposes a single Tauri command,
`detect_project_zomboid`, returning a serializable result with optional
`profile`, `installPath`, and `workshopPath` fields. The command reads Steam
roots from the Windows registry and common fallback locations, parses Steam
library, app manifest, and login-user VDF files, and verifies the game and
Workshop directories. React invokes the command once on mount and renders
Chakra UI states for checking, found, not found, and error.

**Tech Stack:** Tauri 2, Rust 2021, `winreg` on Windows, React 19, TypeScript,
Chakra UI, Vite.

**Spec:** `docs/superpowers/specs/2026-10-01-project-zomboid-path-detection-design.md`

## Global Constraints

- Windows desktop builds only for the first version.
- Detect the game automatically when the application starts.
- Do not add manual path selection, launching, game management, or persistence.
- Use Chakra UI for the dashboard surface.
- Missing Steam registry keys, configs, and manifests are normal not-found candidates.

---

### Task 1: Add Rust Steam VDF discovery behavior

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces `parse_vdf_pairs(content: &str) -> Vec<(String, String)>`.
- Produces `vdf_value(content: &str, key: &str) -> Option<String>`.
- Produces `steam_profile_from_loginusers(content: &str) -> Option<SteamProfile>`.
- Produces `find_project_zomboid_paths() -> Result<ProjectZomboidPaths, String>`.
- Keeps the public Tauri command for Task 2 as `detect_project_zomboid() -> Result<ProjectZomboidPaths, String>`.

- [ ] **Step 1: Write failing Rust tests for escaped VDF pairs**

Add a test module that expects:

```rust
#[test]
fn parses_escaped_windows_paths() {
    let content = r#""path" "D:\\SteamLibrary"
"label" ""
"contentid" "123""#;

    assert_eq!(
        vdf_value(content, "path"),
        Some(r#"D:\SteamLibrary"#.to_string())
    );
}
```

- [ ] **Step 2: Run the focused test and verify the expected failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml parses_escaped_windows_paths
```

Expected: compilation or test failure because the parser functions do not
exist yet.

- [ ] **Step 3: Add the minimal VDF tokenizer and value lookup**

Implement a small quoted-token parser in `src-tauri/src/lib.rs` that:

- Reads quoted strings line by line.
- Treats `\\` as a literal backslash and `\"` as a literal quote.
- Returns adjacent quoted tokens as key/value pairs.
- Lets `vdf_value` return the first value for a requested key.

Do not add a general VDF dependency for the two Steam file shapes needed here.

- [ ] **Step 4: Run the parser test and verify it passes**

Run the same focused `cargo test` command. Expected: PASS.

- [ ] **Step 5: Write failing tests for Steam library and manifest discovery**

Add tests covering:

```rust
#[test]
fn finds_game_directory_from_library_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("SteamLibrary");
    let manifest_dir = library.join("steamapps");
    let game_dir = library.join("steamapps/common/ProjectZomboid");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::write(
        manifest_dir.join("appmanifest_108600.acf"),
        r#""AppState"
{
    "appid" "108600"
    "installdir" "ProjectZomboid"
}"#,
    )
    .unwrap();

    assert_eq!(
        game_path_from_library(&library),
        Some(game_dir.to_string_lossy().into_owned())
    );
}
```

Also test that a missing game directory returns `None`.

- [ ] **Step 6: Add the temporary-directory test dependency**

Add `tempfile` under `[dev-dependencies]` in `src-tauri/Cargo.toml`, then run
the new focused tests to confirm they fail for the missing helper.

- [ ] **Step 7: Implement manifest and library path helpers**

Implement:

- `game_path_from_library(library: &Path) -> Option<String>`, which reads
  `steamapps/appmanifest_108600.acf`, gets `installdir`, and returns the
  verified `steamapps/common/<installdir>` path.
- `library_paths_from_config(steam_root: &Path) -> Vec<PathBuf>`, which reads
  `steamapps/libraryfolders.vdf`, extracts all `path` values, and includes the
  root itself.
- `find_project_zomboid_paths()`, which deduplicates registry/default roots,
  checks each Steam root, and returns the first verified game, Workshop, and
  local profile data.

On Windows, use `winreg` to read `SteamPath`/`InstallPath` from the current
user and common machine registry locations. Add `ProgramFiles(x86)\Steam` and
`ProgramFiles\Steam` when present. On non-Windows builds, return a clear
platform error so the crate remains compilable.

- [ ] **Step 8: Run all Rust tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS with parser, manifest, and missing-directory coverage.

### Task 2: Expose the detector through Tauri

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces the registered command:

```rust
#[tauri::command]
fn detect_project_zomboid() -> Result<ProjectZomboidPaths, String>
```

- [ ] **Step 1: Add a command-level test**

Keep filesystem behavior in the tested helper from Task 1 and add a small
compile-time integration point by registering `detect_project_zomboid` in the
Tauri handler. The command delegates directly to
`find_project_zomboid_paths()`.

- [ ] **Step 2: Register the command and remove the unused starter command**

Update `tauri::generate_handler!` to contain only
`detect_project_zomboid`, preserving the opener plugin and generated context.

- [ ] **Step 3: Run Rust verification**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: both commands pass without warnings caused by the new code.

### Task 3: Replace the starter React screen with a Chakra dashboard

**Files:**
- Modify: `package.json`
- Modify: `src/main.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.css`

**Interfaces:**
- Consumes the Tauri command through:

```ts
invoke<ProjectZomboidPaths>("detect_project_zomboid")
```

- [ ] **Step 1: Add Chakra UI dependencies**

Install `@chakra-ui/react` and `@emotion/react`, keeping the existing React and
Tauri dependencies intact.

- [ ] **Step 2: Add a failing frontend type/build expectation**

Update the app entry to reference Chakra's provider and update `App.tsx` to
reference the four detector states. Run:

```powershell
npm run build
```

Expected: failure until the provider, components, and invoke-driven state are
implemented consistently.

- [ ] **Step 3: Add Chakra provider setup**

Wrap the app in `ChakraProvider` using Chakra's default system in
`src/main.tsx`.

- [ ] **Step 4: Implement startup detection and state rendering**

In `src/App.tsx`:

- Start in `checking`.
- Invoke `detect_project_zomboid` inside a mount effect.
- Set `found` with the returned path, `notFound` for `null`, and `error` for
  rejected commands.
- Render a compact dashboard with a title, status badge, local Steam profile,
  and separate game/workshop path fields.

- [ ] **Step 5: Replace starter CSS**

Remove Vite/React starter styles and keep only global body sizing, margin,
font, and background defaults that support the Chakra layout.

- [ ] **Step 6: Build the frontend**

Run:

```powershell
npm run build
```

Expected: PASS with the Chakra-powered bundle emitted to `dist`.

### Task 4: End-to-end verification and cleanup

**Files:**
- Inspect: `src-tauri/src/lib.rs`
- Inspect: `src/App.tsx`
- Inspect: `src/main.tsx`
- Inspect: `src/App.css`
- Inspect: `package.json`

- [ ] **Step 1: Run Rust formatting and tests**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: formatting check and all Rust tests pass.

- [ ] **Step 2: Run the frontend build**

Run `npm run build` and confirm there are no TypeScript errors or Vite errors.

- [ ] **Step 3: Run the Tauri build check**

Run:

```powershell
npm run tauri -- info
```

Confirm the Tauri CLI can load the project configuration. If the full desktop
runtime is available, run `npm run tauri dev` and visually confirm the initial
state transitions to either a found path or a not-found status.

- [ ] **Step 4: Review the diff for scope**

Confirm every modified line belongs to Steam detection, the Tauri command,
Chakra dashboard, or the required dependency/configuration changes. Leave
unrelated starter assets untouched.
