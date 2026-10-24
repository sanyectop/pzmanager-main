# Project Zomboid Steam Path Detection

## Goal

Turn the starter Tauri + React application into a small Windows dashboard that
shows whether Steam has Project Zomboid installed and, when found, displays its
installation path, the Steam Workshop content path for AppID `108600`, and the
local Steam account information recorded by the client.

## Scope

- Windows desktop builds only for the first version.
- Detect the game automatically when the application starts.
- Do not add manual path selection, launching, game management, or persistence.
- Use Chakra UI for the dashboard surface.
- Provide user-triggered, silent ZombieBuddy installation in Both mode when its
  Workshop files are already downloaded.

## Architecture

The Rust side owns local Steam discovery. A Tauri command named
`detect_project_zomboid` returns an object with optional `installPath` and
`workshopPath` fields plus an optional `profile` object containing `steamId`,
`accountName`, and `personaName`:

1. Read Steam installation roots from the Windows registry when available.
2. Add common Steam installation directories as fallbacks.
3. Read each root's `steamapps/libraryfolders.vdf`.
4. Collect Steam library paths and inspect each library's
   `steamapps/appmanifest_108600.acf`.
5. Read the manifest's `installdir` and verify the resulting
   `steamapps/common/<installdir>` directory exists.
6. Check `steamapps/workshop/content/108600` in each library and return it only
   when the directory exists.
7. Read `config/loginusers.vdf` and select the `MostRecent` account, falling
   back to the first complete account record when no account is marked recent.
8. Return both discovered paths and the optional profile; each may be `null`.

The frontend invokes the command once from a startup effect. It renders four
states: checking, found, not found, and error. The found state displays the
game installation path and workshop path independently in monospace fields;
the other states display concise status messages for each location. The account
card displays the local Steam nickname, account name, and SteamID, with a
Chakra Avatar fallback based on the nickname rather than claiming a remote
avatar image.

The detector also reports ZombieBuddy availability and installation status.
The installer is not run on startup: a user must click the Both install
control. The Rust installer copies `zbNative.dll` and `ZombieBuddy.jar` from
the subscribed Workshop mod, then applies the official Both configuration to
`ProjectZomboid64.json`, `ProjectZomboid64.bat`, and the most-recent account's
Steam `localconfig.vdf`. It validates all target content before writing and
backs up existing targets under `.pzmanager-backups/<timestamp>`. It refuses
to install if a Steam configuration change is needed while Steam is running.
An installation is considered complete based on the copied runtime files and
all three launch configurations, even if the Workshop source is later removed.

## Error Handling

- Missing registry keys, Steam config files, or game manifests are treated as
  normal "not found" candidates while other libraries are checked.
- Unexpected filesystem or registry failures are returned as a command error.
- The frontend keeps the error message visible without attempting retries or
  adding a second workflow in this version.

## Testing

- Unit tests cover parsing quoted VDF key/value pairs with escaped Windows
  paths.
- Unit tests cover extracting Steam library paths and manifest install
  directories.
- Unit tests cover returning a verified game directory from a temporary
  library layout.
- Unit tests cover returning the existing Project Zomboid Workshop directory
  and ignoring a missing one.
- Unit tests cover selecting the most recent account from `loginusers.vdf`.
- Unit tests cover SteamID64-to-`userdata` AccountID conversion.
- Unit tests cover idempotent Both configuration, installation backups,
  already-installed detection without Workshop source files, and ensuring
  malformed Steam configuration does not cause a partial install.
- `npm run build` verifies the React/Chakra TypeScript bundle.
- `cargo test` and `cargo check` verify the Rust command and platform-gated
  implementation.

## Files

- `src-tauri/Cargo.toml`: add the Windows registry dependency.
- `src-tauri/src/lib.rs`: add Steam discovery helpers, tests, and the Tauri
  command.
- `src/App.tsx`: replace the starter screen with the dashboard.
- `src/App.css`: remove starter styling and retain only global page defaults.
- `src/main.tsx`: provide Chakra UI's system provider.
- `package.json`: add Chakra UI dependencies.
