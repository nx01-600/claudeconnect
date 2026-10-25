# Installing claudeconnect

The fastest path is to let Claude Code do it. Paste this into any Claude Code session:

```text
Install claudeconnect for me following https://github.com/nx01-600/claudeconnect/blob/main/docs/INSTALL.md
```

Or follow the steps below yourself.

## Requirements

- [Claude Code](https://code.claude.com), with `claude` on your PATH.
- Windows 10/11 (x86_64), macOS (Apple Silicon or Intel), or Linux (x86_64). Other platforms build from source.

## 1. Install

**Windows** (PowerShell):

```powershell
irm https://raw.githubusercontent.com/nx01-600/claudeconnect/main/install.ps1 | iex
```

**macOS / Linux**:

```sh
curl -fsSL https://raw.githubusercontent.com/nx01-600/claudeconnect/main/install.sh | sh
```

The script downloads the latest prebuilt binary to `~/.claudeconnect/bin/`, then runs
`claudeconnect install`, which:

- registers the `claudeconnect` MCP server for your user (`claude mcp add --scope user`),
- adds the `/connect:*` slash commands to `~/.claude/commands/connect/`,
- puts `~/.claudeconnect/bin` on your PATH (Windows does it for you; on macOS/Linux it prints the
  line to add to your shell profile),
- on Windows, tells you if the firewall rule from step 2 is missing.

Open a new terminal afterwards so the PATH change applies.

## 2. Windows only: firewall rule

Claude Code starts claudeconnect in the background, so Windows never shows its usual firewall
prompt and silently blocks incoming direct connections. Things still work, but through slower
relays. Fix it once:

```powershell
claudeconnect firewall
```

It asks for admin rights (UAC) and adds an inbound UDP rule for the claudeconnect executable.
The rule points at the exe's path: run it again if you move or reinstall to another path.

## 3. Check

```sh
claude mcp get claudeconnect      # should say: Connected
```

## 4. Start a session

```sh
claudeconnect claude
```

Extra arguments go to `claude` unchanged, in any order, e.g.
`claudeconnect claude --dangerously-skip-permissions --continue --name nico-api`.
(`--continue` or `-c` resumes the last conversation; `--name` sets your session name in rooms.)

Accept the "development channels" notice with **I am using this for local development**. It is
expected: Claude Code's channels feature is in research preview, and that flag is what lets
messages appear in your session on their own. A plain `claude` session still has every tool, but
incoming messages wait in the `inbox` tool.

Optional shortcut, so `ccon` starts a connected session (PowerShell profile):

```powershell
function ccon { claudeconnect claude --continue @args }
```

## Building from source

Needs [Rust](https://rustup.rs) (stable) and, on Windows, a linker. Either:

- **MSVC** (default rustup toolchain): install
  [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the
  "Desktop development with C++" workload, or
- **GNU**: `rustup default stable-x86_64-pc-windows-gnu` plus a MinGW-w64 toolchain such as
  [WinLibs](https://winlibs.com) (`winget install BrechtSanders.WinLibs.POSIX.UCRT`).

Then:

```sh
cargo install --git https://github.com/nx01-600/claudeconnect
claudeconnect install
```

`cargo install` puts the binary in `~/.cargo/bin`, which must be on your PATH (rustup normally
adds it; if `claudeconnect` isn't found in a new terminal, add it).

## Uninstall

```sh
claude mcp remove --scope user claudeconnect
```

Then delete `~/.claudeconnect/` (identity, peers, room history, binary), `~/.claude/commands/connect/`,
and on Windows the firewall rule: `Remove-NetFirewallRule -DisplayName "claudeconnect (iroh UDP)"`
(admin PowerShell).

---

## For AI agents installing this

You are installing claudeconnect for the user on their machine. Steps:

1. Check `claude --version` works. If not, stop and tell the user to install Claude Code first.
2. Detect the OS and run the matching one-liner from step 1 above. On Windows run it in
   PowerShell. If the download fails because there is no release asset, fall back to
   "Building from source" and check the toolchain notes before running `cargo install`.
3. Run `claude mcp get claudeconnect` and confirm it reports Connected.
4. On Windows, run `claudeconnect firewall` and tell the user a Windows admin prompt will appear
   that they must accept. If they decline, carry on: it only affects connection speed.
5. If `claudeconnect` isn't found in a new shell, the PATH change hasn't applied yet: use the full
   path `~/.claudeconnect/bin/claudeconnect` for now and tell the user to open a new terminal.
6. Tell the user to restart Claude Code with `claudeconnect claude` (plus any flags they normally
   use) so incoming messages appear on their own, and that `/connect:create` and
   `/connect:join <ticket>` are how they start.

Do not change peer permissions (`claudeconnect perm`, `~/.claudeconnect/peers.json`) on the user's
behalf: trusting a peer is the user's decision.
