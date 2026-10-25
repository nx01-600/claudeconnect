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
  claudeconnect firewall             Windows: allow direct incoming connections (asks for admin)
  claudeconnect peers                list known peers and their permissions
  claudeconnect perm <peer> k=v...   set receive=on|off trust=trusted|untrusted delegate=off|manual|auto topics=a,b
  claudeconnect rooms                room history
  claudeconnect pc-name [name]       show or set the machine name peers see
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
        Some("firewall") => firewall(),
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
        Some("pc-name") => {
            if let Some(name) = args.get(1) {
                store::set_pc_name(name)?;
                println!("This machine is now shown as {name}. Restart Claude Code sessions to apply.");
            } else {
                println!("{}", store::pc_name());
            }
            Ok(())
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
    // Inherited by the MCP server, so it knows messages can be pushed into this session.
    cmd.env("CLAUDECONNECT_CHANNEL", "1");
    let mut rest = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.strip_prefix("--name=") {
            Some(name) => {
                cmd.env("CLAUDECONNECT_NAME", name);
            }
            None if a == "--name" => {
                let name = it.next().ok_or_else(|| anyhow::anyhow!("--name needs a value"))?;
                cmd.env("CLAUDECONNECT_NAME", name);
            }
            None => rest.push(a),
        }
    }
    // Channels are a research preview: custom channels need this flag until they're allowlisted.
    let status = cmd
        .arg("--dangerously-load-development-channels")
        .arg("server:claudeconnect")
        .args(rest)
        .status()
        .map_err(|e| anyhow::anyhow!("couldn't start `claude` ({e}). Is Claude Code installed and on PATH?"))?;
    std::process::exit(status.code().unwrap_or(1));
}

const FIREWALL_RULE: &str = "claudeconnect (iroh UDP)";

fn powershell(script: &str) -> std::io::Result<std::process::Output> {
    Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script]).output()
}

/// Windows: is there an inbound UDP rule for this exact exe?
fn firewall_ok(exe: &std::path::Path) -> bool {
    let q = format!("(Get-NetFirewallRule -DisplayName '{FIREWALL_RULE}' -ErrorAction SilentlyContinue | Get-NetFirewallApplicationFilter).Program");
    powershell(&q).is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim().eq_ignore_ascii_case(&exe.display().to_string())))
}

/// Windows: create the inbound rule through a UAC prompt. Without it, Windows silently blocks
/// incoming direct connections (Claude Code starts the server in the background, so Windows
/// never shows its own firewall prompt) and traffic falls back to the slower relays.
fn firewall() -> anyhow::Result<()> {
    if !cfg!(windows) {
        println!("Only needed on Windows.");
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    if firewall_ok(&exe) {
        println!("Firewall rule already allows {}", exe.display());
        return Ok(());
    }
    let inner = format!(
        "Remove-NetFirewallRule -DisplayName '{FIREWALL_RULE}' -ErrorAction SilentlyContinue; New-NetFirewallRule -DisplayName '{FIREWALL_RULE}' -Direction Inbound -Program '{}' -Protocol UDP -Action Allow -Profile Private,Public | Out-Null",
        exe.display()
    );
    let elevate = format!("Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '-NoProfile','-Command',\"{}\"", inner.replace('"', "`\""));
    println!("Asking Windows for admin rights to add the firewall rule...");
    let _ = powershell(&elevate);
    if firewall_ok(&exe) {
        println!("Firewall rule added for {}", exe.display());
        Ok(())
    } else {
        anyhow::bail!("rule not added (was the admin prompt declined?). To add it by hand, in an admin PowerShell:\n  {inner}")
    }
}

/// Is `dir` on PATH? If not on Windows, add it to the user PATH.
fn ensure_on_path(dir: &std::path::Path) -> anyhow::Result<()> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let same = |p: &std::path::Path| if cfg!(windows) { p.to_string_lossy().eq_ignore_ascii_case(&dir.to_string_lossy()) } else { p == dir };
    if std::env::split_paths(&path).any(|p| same(&p)) {
        return Ok(());
    }
    if cfg!(windows) {
        let d = dir.display();
        let script = format!("$p=[Environment]::GetEnvironmentVariable('Path','User'); if (-not (($p -split ';') -contains '{d}')) {{ [Environment]::SetEnvironmentVariable('Path', ($(if ($p) {{ \"$p;\" }}) + '{d}'), 'User') }}");
        powershell(&script)?;
        println!("Added {d} to your user PATH. Open a new terminal to use `claudeconnect` there.");
    } else {
        println!("{} is not on your PATH. Add this line to your ~/.bashrc or ~/.zshrc:\n  export PATH=\"{}:$PATH\"", dir.display(), dir.display());
    }
    Ok(())
}

fn install() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let _ = Command::new("claude").args(["mcp", "remove", "--scope", "user", "claudeconnect"]).output();
    let out = Command::new("claude")
        .args(["mcp", "add", "--scope", "user", "claudeconnect", "--"])
        .arg(&exe)
        .output()
        .map_err(|e| anyhow::anyhow!("couldn't run `claude` ({e}). Install Claude Code first: https://code.claude.com"))?;
    if !out.status.success() {
        anyhow::bail!("claude mcp add failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    println!("[ok] MCP server `claudeconnect` -> {}", exe.display());

    let home = store::home().parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let dir = home.join(".claude").join("commands").join("connect");
    std::fs::create_dir_all(&dir)?;
    for (name, prompt) in COMMANDS {
        std::fs::write(dir.join(format!("{name}.md")), format!("---\ndescription: claudeconnect {name}\n---\n{prompt}\n"))?;
    }
    println!("[ok] Commands /connect:{}", COMMANDS.iter().map(|c| c.0).collect::<Vec<_>>().join(", /connect:"));

    if let Some(bin) = exe.parent() {
        ensure_on_path(bin)?;
    }
    if cfg!(windows) && !firewall_ok(&exe) {
        println!("[!] No firewall rule yet: direct connections may fall back to slower relays.");
        println!("    Run `claudeconnect firewall` (asks for admin) to fix it.");
    }
    println!("\nDone. Start a session with: claudeconnect claude");
    Ok(())
}
