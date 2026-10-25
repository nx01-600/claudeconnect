mod envelope;
mod mcp;
mod net;
mod store;

use std::process::Command;

const HELP: &str = "claudeconnect: peer-to-peer rooms for Claude Code sessions

USAGE:
  claudeconnect install              register the MCP server and the /connect:* commands for your user
  claudeconnect claude [--name N] [claude args...]
                                     start Claude Code with claudeconnect messages enabled
  claudeconnect peers                list known peers and their permissions
  claudeconnect perm <peer> k=v...   set receive=on|off trust=trusted|untrusted delegate=off|manual|auto topics=a,b
  claudeconnect rooms                room history
  claudeconnect mcp                  run the MCP server (Claude Code does this for you)";

const COMMANDS: &[(&str, &str)] = &[
    ("create", "Create a claudeconnect room named \"$ARGUMENTS\" (or a default name if empty) with room_create, then show me the ticket."),
    ("join", "Join the claudeconnect room with this ticket using room_join: $ARGUMENTS"),
    ("rejoin", "Rejoin the claudeconnect room \"$ARGUMENTS\" with room_rejoin. If no name was given, list the history with rooms and ask me which one."),
    ("peers", "Show the claudeconnect peers tool output as a short table."),
    ("send", "Send this to my claudeconnect room with the send tool, exactly as written: $ARGUMENTS"),
    ("perm", "Change claudeconnect permissions with the perm tool. Request: $ARGUMENTS"),
    ("leave", "Leave the claudeconnect room \"$ARGUMENTS\" with room_leave."),
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("mcp") => mcp::run().await,
        Some("claude") => launch(&args[1..]),
        Some("install") => install(),
        Some("peers") => {
            for (id, p) in store::load_peers() {
                println!("{:<12} {:<24} receive={} trust={:?} delegate={:?} topics={}", &id[..10.min(id.len())], p.name, p.receive, p.trust, p.effective_delegate(), p.topics.join(","));
            }
            Ok(())
        }
        Some("perm") => {
            let [peer, settings @ ..] = &args[1..] else { anyhow::bail!("usage: claudeconnect perm <peer> key=value...") };
            let settings: Vec<(String, String)> = settings
                .iter()
                .map(|s| s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).ok_or_else(|| anyhow::anyhow!("expected key=value, got {s}")))
                .collect::<anyhow::Result<_>>()?;
            let mut peers = store::load_peers();
            let id = store::find_peer(&peers, peer)?;
            let p = peers.get_mut(&id).expect("found above");
            p.apply(&settings)?;
            println!("{}: receive={} trust={:?} delegate={:?}", p.name, p.receive, p.trust, p.effective_delegate());
            store::save_peers(&peers)
        }
        Some("rooms") => {
            for (id, r) in store::load_rooms() {
                let names: Vec<_> = r.members.iter().map(|(m, _)| m.name.as_str()).collect();
                println!("{id}  {:<20} session={:<24} members={}", r.name, r.session, names.join(", "));
            }
            Ok(())
        }
        _ => {
            println!("{HELP}");
            Ok(())
        }
    }
}

/// Start `claude` with the development-channel flag that lets claudeconnect push messages in.
fn launch(args: &[String]) -> anyhow::Result<()> {
    let mut cmd = Command::new("claude");
    let mut rest = args;
    if let [flag, name, tail @ ..] = args {
        if flag == "--name" {
            cmd.env("CLAUDECONNECT_NAME", name);
            rest = tail;
        }
    }
    // Channels are a research preview: custom channels need this flag until they're allowlisted.
    let status = cmd.arg("--dangerously-load-development-channels").arg("server:claudeconnect").args(rest).status()?;
    std::process::exit(status.code().unwrap_or(1));
}

fn install() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let _ = Command::new("claude").args(["mcp", "remove", "--scope", "user", "claudeconnect"]).output();
    let out = Command::new("claude").args(["mcp", "add", "--scope", "user", "claudeconnect", "--"]).arg(&exe).output()?;
    if !out.status.success() {
        anyhow::bail!("claude mcp add failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let home = store::home().parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let dir = home.join(".claude").join("commands").join("connect");
    std::fs::create_dir_all(&dir)?;
    for (name, prompt) in COMMANDS {
        std::fs::write(dir.join(format!("{name}.md")), format!("---\ndescription: claudeconnect {name}\n---\n{prompt}\n"))?;
    }
    println!("Registered MCP server `claudeconnect` -> {}", exe.display());
    println!("Added /connect:{} commands", COMMANDS.iter().map(|c| c.0).collect::<Vec<_>>().join(", /connect:"));
    println!("Start a session with: claudeconnect claude");
    Ok(())
}
