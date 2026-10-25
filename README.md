# claudeconnect

Peer-to-peer rooms for Claude Code sessions running on different machines.

Two people, two computers, two Claude Code sessions: claudeconnect lets those sessions
talk to each other directly, without a central server and without depending on the
Claude account behind each session.

- **Direct connections** over [iroh](https://iroh.computer) (NAT hole punching; public relays
  are used only to find each other, or as an encrypted fallback when a direct path is impossible).
- **Rooms** joined by pasting a ticket, with a name and machine id for every session.
- **Share** text, images, context, memories, and custom topics any plugin can publish.
- **Live status, heads-up warnings, and a local journal** of everything exchanged.
- **Delegation with per-peer permissions**: off, manual approval, or automatic.
- **Trust toggle** per peer, with strict prompt-injection handling for untrusted peers.

Only information travels between machines. A remote session can never edit files or run
commands on your machine: your own Claude Code agent does all the work, with your own
permissions.

> Status: early development.

## Install

Requires [Rust](https://rustup.rs) and Claude Code.

```sh
cargo install --git https://github.com/nx01-600/claudeconnect
claudeconnect install
```

`install` registers the `claudeconnect` MCP server for your user and adds `/connect:*` slash commands.

## Use

Start Claude Code through the launcher (extra arguments go straight to `claude`):

```sh
claudeconnect claude
claudeconnect claude --name nico-api --dangerously-skip-permissions
```

Channels, the Claude Code feature that lets claudeconnect push messages into a running session,
are a research preview. Until custom channels are allowlisted, sessions must start with
`--dangerously-load-development-channels server:claudeconnect`, which the launcher adds for you.
Without it the tools still work, and incoming messages wait in the `inbox` tool.

Then just ask Claude ("create a room", "join this ticket: cc1...", "tell the room the API is
ready", "delegate the migration to ana-backend"), or use the commands:

| Command | What it does |
|---|---|
| `/connect:create [name]` | create a room; the ticket is copied to your clipboard |
| `/connect:join <ticket>` | join a room |
| `/connect:rejoin [name]` | rejoin a room from history |
| `/connect:peers` | who is in the room, their live status, and direct vs relayed link |
| `/connect:send <text>` | message the room |
| `/connect:perm <request>` | change what a peer may do |
| `/connect:leave [name]` | leave a room |

## Permissions

Each peer gets its own settings, stored per machine in `~/.claudeconnect/peers.json`:

| Setting | Values | Default |
|---|---|---|
| `receive` | `on`, `off` | `on` |
| `trust` | `trusted`, `untrusted` | `untrusted` |
| `delegate` | `off`, `manual`, `auto` | `manual` |
| `topics` | comma list of accepted `custom` topics, empty = all | empty |

- Untrusted peers' messages reach Claude marked as data, never as instructions, and their
  delegations always need manual approval.
- Manual delegation opens an approval dialog that you answer, not Claude.
- Claude can lower permissions on its own. Raising them asks you in a dialog. From a terminal,
  `claudeconnect perm <peer> trust=trusted delegate=auto` changes them directly.

## Journal

Every project gets a `.claudeconnect/` folder with `journal.jsonl` (everything sent and received)
and `files/` (received attachments). It is added to `.git/info/exclude` automatically. Turn it off
with the `journal` tool.

## How it works

Each Claude Code session runs its own `claudeconnect` process with its own iroh endpoint. A room
ticket carries the room id, a secret, and the host's address. Joining sessions prove the secret,
receive the member list, and open a direct QUIC connection to every member. Messages are JSON
envelopes (`text`, `image`, `context`, `memory`, `status`, `warning`, `change`, `delegate`,
`result`, `custom` + `topic`) with inline attachments up to 25 MB.

## License

MIT
