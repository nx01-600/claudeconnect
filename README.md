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
