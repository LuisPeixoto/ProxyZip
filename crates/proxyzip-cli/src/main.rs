use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use proxyzip_core::{FlowEvent, FlowState, FlowStore, FlowSummary};
use proxyzip_proxy::{ProxyConfig, ProxyServer, ProxySettings, wildcard_match};
use proxyzip_rules::{ReverseRule, RuleSet, SocksListener, Throttle, ThrottleProfile, UpstreamKind, UpstreamProxy};
use proxyzip_tls::CertificateAuthority;

#[derive(Parser)]
#[command(name = "proxyzip", version, about = "ProxyZip — capture, inspect and debug HTTP(S) traffic")]
struct Cli {
    /// Data directory (CA, settings). Defaults to the per-user ProxyZip directory.
    #[arg(long, global = true, env = "PROXYZIP_HOME")]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the proxy in the foreground and print traffic as it flows.
    Start(Box<StartArgs>),
    /// Manage the root certificate authority.
    Ca {
        #[command(subcommand)]
        action: CaAction,
    },
    /// Turn the operating-system proxy on/off.
    SystemProxy {
        #[command(subcommand)]
        action: SysAction,
    },
    /// Print shell exports that route a terminal through ProxyZip.
    Env {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(short, long, default_value_t = 9393)]
        port: u16,
    },
    /// Print a HAR file's requests as cURL commands.
    HarToCurl { file: PathBuf },
}

#[derive(clap::Args)]
struct StartArgs {
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(short, long, default_value_t = 9393)]
    port: u16,
    /// Point the OS proxy at ProxyZip while running (restored on exit).
    #[arg(long)]
    system_proxy: bool,
    /// Write everything captured to this HAR file on exit.
    #[arg(long)]
    har: Option<PathBuf>,
    /// Never decrypt these hosts (wildcards allowed). Repeatable.
    #[arg(long = "exclude", value_name = "HOST")]
    exclude: Vec<String>,
    /// Only decrypt these hosts (implies everything else is tunnelled). Repeatable.
    #[arg(long = "include", value_name = "HOST")]
    include: Vec<String>,
    /// Only print flows whose URL matches this wildcard.
    #[arg(long)]
    filter: Option<String>,
    /// Do not verify upstream server certificates.
    #[arg(long)]
    insecure: bool,
    /// Append TLS secrets in NSS key log format (for Wireshark).
    #[arg(long)]
    keylog: Option<PathBuf>,
    /// Load rules (block, map, rewrite, …) from a JSON file. The flags below add to them.
    #[arg(long, value_name = "FILE")]
    rules: Option<PathBuf>,
    /// Also listen for SOCKS5 clients on this port.
    #[arg(long, value_name = "PORT")]
    socks_port: Option<u16>,
    /// Send traffic through another proxy: http://[user:pass@]host:port or socks5://[user:pass@]host:port.
    #[arg(long, value_name = "URL")]
    upstream_proxy: Option<String>,
    /// Expose a server on a local port: PORT=URL, e.g. 8081=https://api.example.com. Repeatable.
    #[arg(long = "reverse", value_name = "PORT=URL")]
    reverse: Vec<String>,
    /// Simulate a slow network: edge, 3g, 4g or wifi-poor.
    #[arg(long, value_name = "PRESET")]
    throttle: Option<String>,
    /// Print request/response headers for every flow.
    #[arg(short, long)]
    verbose: bool,
    /// Do not print flows.
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Subcommand)]
enum CaAction {
    /// Print the certificate file path.
    Path,
    /// Print the certificate (PEM).
    Show,
    /// Trust the certificate in the operating-system store.
    Install,
    /// Remove the certificate from the operating-system store.
    Uninstall,
    /// Show whether the certificate is trusted.
    Status,
    /// Replace the CA with a new one (old one must be uninstalled separately).
    Regenerate,
    /// Install into the booted iOS Simulator(s) (macOS).
    IosSimulator,
}

#[derive(Subcommand)]
enum SysAction {
    On {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(short, long, default_value_t = 9393)]
        port: u16,
    },
    Off,
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("PROXYZIP_LOG").unwrap_or_else(|_| "warn".into()))
        .with_writer(std::io::stderr)
        .init();
    let home = cli.home.unwrap_or_else(proxyzip_core::data_dir);
    match cli.command {
        Command::Start(args) => start(home, *args).await,
        Command::Ca { action } => ca(home, action),
        Command::SystemProxy { action } => match action {
            SysAction::On { host, port } => proxyzip_sysproxy::enable(&host, port),
            SysAction::Off => proxyzip_sysproxy::disable(),
            SysAction::Status => {
                println!("{}", serde_json::to_string_pretty(&proxyzip_sysproxy::status()?)?);
                Ok(())
            }
        },
        Command::Env { host, port } => {
            let ca = CertificateAuthority::load_or_create(&home)?;
            println!("{}", proxyzip_certinstall::terminal_env(&host, port, &ca.cert_path().unwrap()));
            Ok(())
        }
        Command::HarToCurl { file } => {
            let json = std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
            for flow in proxyzip_export::har::import(&json)? {
                println!("{}\n", proxyzip_export::curl::to_curl(&flow));
            }
            Ok(())
        }
    }
}

fn ca(home: PathBuf, action: CaAction) -> Result<()> {
    let ca = CertificateAuthority::load_or_create(&home)?;
    let path = ca.cert_path().unwrap();
    match action {
        CaAction::Path => println!("{}", path.display()),
        CaAction::Show => print!("{}", ca.cert_pem()),
        CaAction::Install => {
            proxyzip_certinstall::install(&path)?;
            println!("Installed {}", path.display());
        }
        CaAction::Uninstall => {
            proxyzip_certinstall::uninstall(&path)?;
            println!("Removed ProxyZip CA from the system store");
        }
        CaAction::Status => {
            let s = proxyzip_certinstall::status(&path);
            println!("{} — {}", if s.installed { "trusted" } else { "not trusted" }, s.detail);
        }
        CaAction::Regenerate => {
            let ca = CertificateAuthority::regenerate(&home)?;
            println!("New CA written to {}", ca.cert_path().unwrap().display());
        }
        CaAction::IosSimulator => {
            proxyzip_certinstall::install_ios_simulator(&path)?;
            println!("Installed into booted iOS Simulator(s)");
        }
    }
    Ok(())
}

async fn start(home: PathBuf, args: StartArgs) -> Result<()> {
    let ca = Arc::new(CertificateAuthority::load_or_create(&home)?);
    let store = FlowStore::default();
    let listen: SocketAddr = format!("{}:{}", args.host, args.port).parse().context("invalid listen address")?;
    let mut settings = ProxySettings { insecure_upstream: args.insecure, ..Default::default() };
    settings.ssl.exclude = args.exclude.clone();
    if !args.include.is_empty() {
        settings.ssl.intercept_all = false;
        settings.ssl.include = args.include.clone();
    }
    settings.rules = Arc::new(build_rules(&args)?);
    let config = ProxyConfig { listen, keylog_path: args.keylog.clone(), ..Default::default() };
    let handle = ProxyServer::start(config, settings, ca.clone(), store.clone()).await?;
    let addr = handle.local_addr;

    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    eprintln!("{} listening on {}", paint("1;35", "⚡ ProxyZip"), paint("1", &format!("http://{addr}")));
    eprintln!("   CA certificate: {}", ca.cert_path().unwrap().display());
    eprintln!("   Download on devices: http://{} (through the proxy)", proxyzip_proxy::CERT_HOST);
    {
        let rules = handle.settings.read().rules.clone();
        if rules.socks.enabled {
            eprintln!("   SOCKS5 listener on {}:{}", addr.ip(), rules.socks.port);
        }
        for r in rules.active_reverse() {
            eprintln!("   Reverse proxy {}:{} → {}", addr.ip(), r.listen_port, r.target);
        }
        if rules.throttle.enabled {
            eprintln!("   Throttling: {}", rules.throttle.profile.name);
        }
        for error in &handle.listener_errors {
            eprintln!("   {} {error}", paint("31", "✗"));
        }
    }
    if !proxyzip_certinstall::status(&ca.cert_path().unwrap()).installed {
        eprintln!("   {} the CA is not trusted yet — run `proxyzip ca install` to inspect HTTPS", paint("33", "!"));
    }

    let mut system_proxy_on = false;
    if args.system_proxy {
        let host = if addr.ip().is_unspecified() { "127.0.0.1".to_string() } else { addr.ip().to_string() };
        match proxyzip_sysproxy::enable(&host, addr.port()) {
            Ok(()) => {
                system_proxy_on = true;
                eprintln!("   System proxy enabled (restored on exit)");
            }
            Err(e) => eprintln!("   {} could not enable system proxy: {e:#}", paint("31", "✗")),
        }
    }
    eprintln!();

    if !args.quiet {
        let mut rx = store.subscribe();
        let filter = args.filter.clone();
        let verbose = args.verbose;
        let store2 = store.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(FlowEvent::Updated { flow }) if is_final(&flow) => {
                        if filter.as_ref().is_some_and(|f| !wildcard_match(f, &flow.url)) {
                            continue;
                        }
                        println!("{}", line(&flow, color));
                        if verbose && let Some(full) = store2.get(flow.id) {
                            for (k, v) in full.request.headers.iter() {
                                println!("    > {k}: {v}");
                            }
                            if let Some(r) = &full.response {
                                for (k, v) in r.headers.iter() {
                                    println!("    < {k}: {v}");
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => eprintln!("… {n} events skipped"),
                    Err(_) => break,
                }
            }
        });
    }

    tokio::signal::ctrl_c().await?;
    eprintln!("\nShutting down…");
    if system_proxy_on && let Err(e) = proxyzip_sysproxy::disable() {
        eprintln!("could not restore system proxy: {e:#}");
    }
    handle.shutdown().await;
    if let Some(path) = args.har {
        let flows = store.all();
        std::fs::write(&path, proxyzip_export::har::export_json(&flows)?)?;
        eprintln!("Wrote {} flows to {}", flows.len(), path.display());
    }
    Ok(())
}

/// Combine the rules file with the command-line shortcuts.
fn build_rules(args: &StartArgs) -> Result<RuleSet> {
    let mut rules = match &args.rules {
        Some(path) => {
            let json = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            RuleSet::from_json(&json).with_context(|| format!("parsing {}", path.display()))?
        }
        None => RuleSet::default(),
    };
    if let Some(port) = args.socks_port {
        rules.socks = SocksListener { enabled: true, port };
    }
    if let Some(url) = &args.upstream_proxy {
        rules.upstream = parse_upstream(url)?;
    }
    for spec in &args.reverse {
        let (port, target) =
            spec.split_once('=').with_context(|| format!("--reverse expects PORT=URL, got {spec:?}"))?;
        let listen_port = port.trim().parse().with_context(|| format!("invalid port in --reverse {spec:?}"))?;
        rules.reverse.push(ReverseRule {
            name: format!("CLI {listen_port}"),
            listen_port,
            target: target.trim().to_string(),
            ..Default::default()
        });
    }
    if let Some(name) = &args.throttle {
        let profile = ThrottleProfile::preset(name).with_context(|| {
            format!("unknown throttle preset {name:?}; use one of {}", ThrottleProfile::PRESETS.join(", "))
        })?;
        rules.throttle = Throttle { enabled: true, profile, matchers: Vec::new() };
    }
    Ok(rules)
}

/// Parse `http://user:pass@host:8080` or `socks5://host:1080`.
fn parse_upstream(spec: &str) -> Result<UpstreamProxy> {
    let url = url::Url::parse(spec).with_context(|| format!("invalid upstream proxy URL {spec:?}"))?;
    let (kind, default_port) = match url.scheme() {
        "http" => (UpstreamKind::Http, 8080),
        "socks5" | "socks5h" => (UpstreamKind::Socks5, 1080),
        other => anyhow::bail!("unsupported upstream proxy scheme {other:?}; use http:// or socks5://"),
    };
    let host = url.host_str().with_context(|| format!("upstream proxy URL {spec:?} has no host"))?;
    let decode = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned();
    Ok(UpstreamProxy {
        enabled: true,
        kind,
        host: host.trim_start_matches('[').trim_end_matches(']').to_string(),
        port: url.port().unwrap_or(default_port),
        username: Some(decode(url.username())).filter(|u| !u.is_empty()),
        password: url.password().map(decode),
        bypass: Vec::new(),
    })
}

fn is_final(f: &FlowSummary) -> bool {
    matches!(f.state, FlowState::Completed | FlowState::Failed | FlowState::WebSocketOpen)
}

fn line(f: &FlowSummary, color: bool) -> String {
    let paint = |code: &str, s: String| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s };
    let status = match (f.status, f.state) {
        (_, FlowState::Failed) => paint("1;31", "ERR".into()),
        (Some(s), _) if s >= 500 => paint("31", s.to_string()),
        (Some(s), _) if s >= 400 => paint("33", s.to_string()),
        (Some(s), _) if s >= 300 => paint("36", s.to_string()),
        (Some(s), _) => paint("32", s.to_string()),
        (None, _) => paint("2", "···".into()),
    };
    let size = f.response_size.map(human_size).unwrap_or_default();
    let dur = f.duration_ms.map(|d| format!("{d}ms")).unwrap_or_default();
    let mut out =
        format!("{status:>3} {:<7} {} {} {}", paint("1", f.method.clone()), f.url, paint("2", size), paint("2", dur));
    if !f.tags.is_empty() {
        out.push_str(&paint("35", format!(" [{}]", f.tags.join(", "))));
    }
    if let Some(e) = &f.error {
        out.push_str(&paint("31", format!("\n    {e}")));
    }
    out
}

fn human_size(b: usize) -> String {
    match b {
        b if b >= 1 << 20 => format!("{:.1}MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1}KB", b as f64 / 1024.0),
        b => format!("{b}B"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_urls() {
        let u = parse_upstream("http://alice:p%40ss@proxy.corp:3128").unwrap();
        assert_eq!((u.kind, u.host.as_str(), u.port), (UpstreamKind::Http, "proxy.corp", 3128));
        assert_eq!((u.username.as_deref(), u.password.as_deref()), (Some("alice"), Some("p@ss")));
        let s = parse_upstream("socks5://127.0.0.1").unwrap();
        assert_eq!((s.kind, s.port, s.username), (UpstreamKind::Socks5, 1080, None));
        assert!(parse_upstream("ftp://x").is_err());
        assert!(parse_upstream("not a url").is_err());
    }

    fn args(extra: &[&str]) -> StartArgs {
        use clap::Parser;
        #[derive(Parser)]
        struct Wrapper {
            #[command(flatten)]
            start: StartArgs,
        }
        Wrapper::parse_from(std::iter::once("proxyzip").chain(extra.iter().copied())).start
    }

    #[test]
    fn flags_become_rules() {
        let rules =
            build_rules(&args(&["--socks-port", "1081", "--reverse", "8081=https://api.dev/v1", "--throttle", "3g"]))
                .unwrap();
        assert!(rules.socks.enabled && rules.socks.port == 1081);
        assert_eq!((rules.reverse[0].listen_port, rules.reverse[0].target.as_str()), (8081, "https://api.dev/v1"));
        assert!(rules.throttle.enabled && rules.throttle.profile.name == "3G");
    }

    #[test]
    fn bad_flags_are_explained() {
        assert!(build_rules(&args(&["--reverse", "nope"])).unwrap_err().to_string().contains("PORT=URL"));
        assert!(build_rules(&args(&["--throttle", "warp"])).unwrap_err().to_string().contains("edge"));
        assert!(build_rules(&args(&["--rules", "/definitely/missing.json"])).is_err());
    }
}
