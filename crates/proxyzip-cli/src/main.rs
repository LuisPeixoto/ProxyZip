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
        /// Shell syntax: sh (bash, zsh), fish, powershell or cmd. Detected from the session by default.
        #[arg(long)]
        shell: Option<String>,
    },
    /// Open a new terminal window whose traffic goes through ProxyZip.
    Terminal {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(short, long, default_value_t = 9393)]
        port: u16,
    },
    /// Let an AI assistant inspect your traffic over MCP (stdio). Add it to Claude, Cursor, VS Code…
    ///
    /// Without options it forwards to a running ProxyZip (desktop app or `proxyzip start --mcp`).
    /// With `--session` it serves a saved session instead, read-only.
    Mcp {
        /// Serve this `.pzsession` or `.har` file.
        #[arg(long, value_name = "FILE")]
        session: Option<PathBuf>,
        /// Endpoint of a running ProxyZip. Found automatically by default.
        #[arg(long)]
        url: Option<String>,
        /// Access token for `--url`.
        #[arg(long, env = "PROXYZIP_MCP_TOKEN")]
        token: Option<String>,
    },
    /// Print a HAR file's requests as cURL commands.
    HarToCurl { file: PathBuf },
    /// Convert a HAR file or a ProxyZip session into another format.
    Export(ExportArgs),
    /// Print the code that sends a captured request, in one of 14 languages.
    Code(CodeArgs),
    /// Serve an OpenAPI specification (or a workspace's mock servers) as a mock API.
    Mock(MockArgs),
    /// Run a replay suite (`.pzsuite`, or a Postman collection) and report what passed. Exits with 1 on a failure.
    Run(RunArgs),
    /// Check captured traffic against an OpenAPI specification. Exits with 1 when something does not conform.
    Validate(ValidateArgs),
}

#[derive(clap::Args)]
struct RunArgs {
    /// A `.pzsuite` file or a Postman v2.1 collection (`.json`).
    suite: PathBuf,
    /// Use the variables of this environment of the suite.
    #[arg(short, long)]
    env: Option<String>,
    /// Set a variable: `--var name=value`. Repeatable.
    #[arg(long = "var", value_name = "NAME=VALUE")]
    vars: Vec<String>,
    /// Send the requests through a running ProxyZip, for example `http://127.0.0.1:9393`, so they appear in its list.
    #[arg(long, value_name = "URL")]
    via_proxy: Option<String>,
    /// Trust ProxyZip's certificate authority (needed with --via-proxy for HTTPS).
    #[arg(long)]
    trust_proxyzip_ca: bool,
    /// Do not verify server certificates.
    #[arg(long)]
    insecure: bool,
    /// Skip the remaining requests after the first failure.
    #[arg(long)]
    bail: bool,
    /// Write a JUnit XML report here.
    #[arg(long, value_name = "FILE")]
    junit: Option<PathBuf>,
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,
    /// Seconds to wait for each response.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

#[derive(clap::Args)]
struct MockArgs {
    /// An OpenAPI 3 file (`.yaml`, `.yml` or `.json`) or a `.pzworkspace` with mock servers.
    spec: PathBuf,
    /// Port to serve on.
    #[arg(short, long, default_value_t = 4010)]
    port: u16,
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Wait this many milliseconds before every answer.
    #[arg(long, default_value_t = 0)]
    delay: u64,
}

#[derive(clap::Args)]
struct ValidateArgs {
    /// The OpenAPI 3 specification (YAML or JSON).
    spec: PathBuf,
    /// A `.har` or `.pzsession` file.
    capture: PathBuf,
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct ExportArgs {
    /// A `.har` file or a `.pzsession` session.
    input: PathBuf,
    /// Output format.
    #[arg(short, long, value_enum)]
    format: ExportFormat,
    /// Write to this file instead of standard output (required for `session`).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Replace secrets (tokens, cookies, passwords) before writing.
    #[arg(long)]
    redact: bool,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ExportFormat {
    Csv,
    Postman,
    Har,
    Session,
}

#[derive(clap::Args)]
struct CodeArgs {
    /// A `.har` file or a `.pzsession` session.
    input: PathBuf,
    /// Language: curl, httpie, powershell, python, javascript, node-axios, go, rust, java, kotlin, swift, csharp, php or ruby.
    #[arg(short, long, default_value = "curl")]
    lang: String,
    /// Which request to use (1 is the first).
    #[arg(short, long, default_value_t = 1)]
    number: usize,
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
    /// Save everything captured to a ProxyZip session file (.pzsession) on exit.
    #[arg(long, value_name = "FILE")]
    save: Option<PathBuf>,
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
    /// Also accept HTTP/3 (UDP) on the ports of every --reverse listener.
    #[arg(long)]
    reverse_h3: bool,
    /// Add the rules of a `.pzworkspace` file (see `docs/workspaces.md`).
    #[arg(long, value_name = "FILE")]
    workspace: Option<PathBuf>,
    /// Remove secrets from what `--har` and `--save` write.
    #[arg(long)]
    redact: bool,
    /// Use HTTP/3 towards servers that advertise it with Alt-Svc (experimental).
    #[arg(long)]
    http3: bool,
    /// Do not remove HTTP/3 advertisements from responses (clients may then bypass the proxy with QUIC).
    #[arg(long)]
    keep_alt_svc: bool,
    /// Serve an MCP endpoint for AI assistants on this local port (http://127.0.0.1:PORT/mcp, token required).
    #[arg(long, value_name = "PORT")]
    mcp: Option<u16>,
    /// Let assistants change rules and clear traffic (needs --mcp).
    #[arg(long, requires = "mcp")]
    mcp_allow_changes: bool,
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
    /// Make Firefox trust ProxyZip in every profile found on this computer.
    Firefox,
    /// Set up an Android emulator or device: list devices, route its traffic, install the certificate.
    Android {
        /// Device serial (see `adb devices`). Required unless `--list` is used.
        #[arg(long)]
        serial: Option<String>,
        /// List connected devices and exit.
        #[arg(long)]
        list: bool,
        /// Send the device's HTTP traffic to this ProxyZip port.
        #[arg(long, value_name = "PORT")]
        proxy: Option<u16>,
        /// Stop routing the device's traffic through ProxyZip.
        #[arg(long)]
        clear_proxy: bool,
        /// Install the CA as a system certificate (emulators and rooted devices).
        #[arg(long)]
        system: bool,
        /// Open Android's installer for a user certificate (any device).
        #[arg(long)]
        user: bool,
    },
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
        Command::Env { host, port, shell } => {
            let ca = CertificateAuthority::load_or_create(&home)?;
            let path = ca.cert_path().unwrap();
            let snippet = match shell {
                Some(name) => {
                    let sh = proxyzip_certinstall::runtime::Shell::from_name(&name)
                        .with_context(|| format!("unknown shell '{name}': use sh, fish, powershell or cmd"))?;
                    proxyzip_certinstall::terminal_env_for(sh, &host, port, &path)
                }
                None => proxyzip_certinstall::terminal_env(&host, port, &path),
            };
            print!("{snippet}");
            Ok(())
        }
        Command::Mcp { session, url, token } => mcp(home, session, url, token).await,
        Command::Terminal { host, port } => {
            let ca = CertificateAuthority::load_or_create(&home)?;
            proxyzip_certinstall::terminal::open(&host, port, &ca.cert_path().unwrap())
        }
        Command::Export(args) => export(args),
        Command::Code(args) => code(args),
        Command::Mock(args) => mock(args).await,
        Command::Run(args) => run_suite(home, args).await,
        Command::Validate(args) => validate(args),
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
        CaAction::Firefox => {
            let report = proxyzip_certinstall::firefox::install(&path)?;
            println!(
                "Configured {} Firefox profile(s) ({} with the certificate imported)",
                report.profiles.len(),
                report.imported
            );
            for note in report.notes {
                println!("{note}");
            }
        }
        CaAction::Android { serial, list, proxy, clear_proxy, system, user } => {
            use proxyzip_certinstall::android;
            if list || serial.is_none() {
                let devices = android::list_devices()?;
                if devices.is_empty() {
                    println!("No Android devices found. Start an emulator or connect a device with USB debugging.");
                }
                for d in devices {
                    println!("{}\t{}\t{}", d.serial, d.state, d.model.unwrap_or_default());
                }
                return Ok(());
            }
            let serial = serial.unwrap();
            if let Some(port) = proxy {
                android::set_proxy(&serial, port)?;
                println!("{serial} now sends HTTP traffic to ProxyZip on port {port}");
            }
            if clear_proxy {
                android::clear_proxy(&serial)?;
                println!("{serial} no longer uses a proxy");
            }
            if system {
                let note = android::install_system_cert(&serial, &path, ca.cert_der().as_ref())?;
                println!("{note}");
            }
            if user {
                let der = std::env::temp_dir().join("proxyzip-ca.crt");
                std::fs::write(&der, ca.cert_der().as_ref() as &[u8])?;
                android::install_user_cert(&serial, &der)?;
                println!("Opened the certificate installer on {serial}");
            }
        }
    }
    Ok(())
}

async fn mcp(home: PathBuf, session: Option<PathBuf>, url: Option<String>, token: Option<String>) -> Result<()> {
    if let Some(path) = session {
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let flows = if path.extension().is_some_and(|e| e == "har") {
            proxyzip_export::har::import(std::str::from_utf8(&bytes)?)?
        } else {
            proxyzip_export::session::read(&bytes)?
        };
        let store = FlowStore::default();
        let count = flows.len();
        for flow in flows {
            store.import(flow);
        }
        let status =
            serde_json::json!({ "running": false, "source": path.display().to_string(), "note": "read-only session" });
        let state = proxyzip_mcp::McpState::new(store).with_status(move || status.clone());
        eprintln!("ProxyZip MCP: serving {count} requests from {}", path.display());
        return proxyzip_mcp::stdio::run(Arc::new(proxyzip_mcp::McpServer::new(Arc::new(state)))).await;
    }
    let endpoint = match (url, token) {
        (Some(url), Some(token)) => proxyzip_mcp::discovery::Endpoint { url, token },
        (Some(_), None) => anyhow::bail!("--url needs --token (or PROXYZIP_MCP_TOKEN)"),
        _ => proxyzip_mcp::bridge::require_endpoint(&home)?,
    };
    proxyzip_mcp::bridge::run(endpoint).await
}

/// Changes to the rule set made by an assistant go through the running proxy.
struct CliRules(Arc<proxyzip_proxy::ProxyHandle>);

impl proxyzip_mcp::RulesBackend for CliRules {
    fn get(&self) -> proxyzip_rules::RuleSet {
        (*self.0.settings.read().rules).clone()
    }

    fn set<'a>(
        &'a self,
        rules: proxyzip_rules::RuleSet,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let errors = self.0.apply_rules(rules).await;
            if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
        })
    }
}

async fn start(home: PathBuf, args: StartArgs) -> Result<()> {
    let ca = Arc::new(CertificateAuthority::load_or_create(&home)?);
    let store = FlowStore::default();
    let listen: SocketAddr = format!("{}:{}", args.host, args.port).parse().context("invalid listen address")?;
    let mut settings = ProxySettings {
        insecure_upstream: args.insecure,
        http3_upstream: args.http3,
        strip_alt_svc: !args.keep_alt_svc,
        ..Default::default()
    };
    settings.ssl.exclude = args.exclude.clone();
    if !args.include.is_empty() {
        settings.ssl.intercept_all = false;
        settings.ssl.include = args.include.clone();
    }
    settings.rules = Arc::new(build_rules(&args)?);
    let config = ProxyConfig { listen, keylog_path: args.keylog.clone(), ..Default::default() };
    let handle = Arc::new(ProxyServer::start(config, settings, ca.clone(), store.clone()).await?);
    let addr = handle.local_addr;
    let mcp_http = match args.mcp {
        Some(port) => {
            let token = proxyzip_mcp::discovery::load_or_create_token(&home)?;
            let status_handle = handle.clone();
            let status_store = store.clone();
            let state = proxyzip_mcp::McpState::new(store.clone())
                .with_rules(Arc::new(CliRules(handle.clone())))
                .with_status(move || {
                    serde_json::json!({
                        "running": true,
                        "proxy": status_handle.local_addr.to_string(),
                        "recording": status_handle.settings.read().recording,
                        "flows": status_store.len(),
                    })
                })
                .allow_changes(args.mcp_allow_changes);
            let server = Arc::new(proxyzip_mcp::McpServer::new(Arc::new(state)));
            let http =
                proxyzip_mcp::http::serve(SocketAddr::from(([127, 0, 0, 1], port)), server, token.clone()).await?;
            proxyzip_mcp::discovery::publish(&home, &proxyzip_mcp::discovery::Endpoint { url: http.url(), token })?;
            Some(http)
        }
        None => None,
    };

    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    eprintln!("{} listening on {}", paint("1;35", "⚡ ProxyZip"), paint("1", &format!("http://{addr}")));
    eprintln!("   CA certificate: {}", ca.cert_path().unwrap().display());
    eprintln!("   Download on devices: http://{} (through the proxy)", proxyzip_proxy::CERT_HOST);
    if let Some(http) = &mcp_http {
        eprintln!(
            "   MCP for AI assistants on {} ({}). Add it with: proxyzip mcp",
            http.url(),
            if args.mcp_allow_changes { "changes allowed" } else { "read-only" }
        );
    }
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
    if let Some(http) = mcp_http {
        http.shutdown();
        proxyzip_mcp::discovery::unpublish(&home);
    }
    match Arc::try_unwrap(handle) {
        Ok(h) => h.shutdown().await,
        Err(_) => tracing::debug!("proxy handle still shared at exit"),
    }
    let finished = |flows: Vec<proxyzip_core::Flow>| {
        if args.redact { proxyzip_export::redact::redacted(&flows, &Default::default()) } else { flows }
    };
    if let Some(path) = args.har {
        let flows = finished(store.all());
        std::fs::write(&path, proxyzip_export::har::export_json(&flows)?)?;
        eprintln!("Wrote {} flows to {}", flows.len(), path.display());
    }
    if let Some(path) = args.save {
        let flows = finished(store.all());
        std::fs::write(&path, proxyzip_export::session::write(&flows)?)?;
        eprintln!("Saved {} flows to {}", flows.len(), path.display());
    }
    Ok(())
}

/// Read flows from a `.pzsession` or a HAR file, judging by the extension.
fn load_flows(path: &std::path::Path) -> Result<Vec<proxyzip_core::Flow>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let is_session = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pzsession"));
    if is_session {
        proxyzip_export::session::read(&bytes)
    } else {
        proxyzip_export::har::import(&String::from_utf8_lossy(&bytes))
    }
}

fn export(args: ExportArgs) -> Result<()> {
    let mut flows = load_flows(&args.input)?;
    if args.redact {
        flows = proxyzip_export::redact::redacted(&flows, &Default::default());
    }
    let text = match args.format {
        ExportFormat::Csv => proxyzip_export::csv::export(&flows),
        ExportFormat::Har => proxyzip_export::har::export_json(&flows)?,
        ExportFormat::Postman => {
            let name = args.input.file_stem().and_then(|s| s.to_str()).unwrap_or("ProxyZip").to_string();
            proxyzip_export::postman::export_json(&name, &flows)?
        }
        ExportFormat::Session => {
            let output = args.output.as_ref().context("the session format is binary: choose a file with --output")?;
            std::fs::write(output, proxyzip_export::session::write(&flows)?)?;
            eprintln!("Wrote {} flows to {}", flows.len(), output.display());
            return Ok(());
        }
    };
    match &args.output {
        Some(path) => {
            std::fs::write(path, text)?;
            eprintln!("Wrote {} flows to {}", flows.len(), path.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}

async fn mock(args: MockArgs) -> Result<()> {
    use proxyzip_rules::MockServer;
    let text = std::fs::read_to_string(&args.spec).with_context(|| format!("reading {}", args.spec.display()))?;
    let is_workspace = args.spec.extension().is_some_and(|e| e.eq_ignore_ascii_case("pzworkspace"));
    let mut servers: Vec<MockServer> = if is_workspace {
        proxyzip_export::workspace::read(&text)?.rules.mocks
    } else {
        let spec = proxyzip_openapi::Spec::parse(&text)?;
        vec![spec.to_mock_server(&spec.title(), "", Some(args.port))]
    };
    anyhow::ensure!(!servers.is_empty(), "{} has no mock servers", args.spec.display());
    for s in servers.iter_mut() {
        s.listen_port = s.listen_port.or(Some(args.port));
        for r in s.routes.iter_mut() {
            r.delay_ms = r.delay_ms.max(args.delay);
        }
    }
    let ca = Arc::new(CertificateAuthority::generate()?);
    let store = FlowStore::default();
    let listen: SocketAddr = format!("{}:0", args.host).parse().context("invalid --host")?;
    let rules = RuleSet { mocks: servers.clone(), ..Default::default() };
    let settings = ProxySettings { rules: Arc::new(rules), ..Default::default() };
    let handle = ProxyServer::start(ProxyConfig { listen, ..Default::default() }, settings, ca, store.clone()).await?;
    if let Some(error) = handle.listener_errors.first() {
        anyhow::bail!("{error}");
    }
    for s in &servers {
        let port = s.listen_port.unwrap_or(args.port);
        eprintln!("⚡ Mock \"{}\" on http://{}:{port}", s.name, args.host);
        for r in s.routes.iter().filter(|r| r.enabled) {
            eprintln!("   {:<7} {}  → {}", if r.method.is_empty() { "ANY" } else { &r.method }, r.path, r.status);
        }
    }
    eprintln!("Press Ctrl+C to stop.\n");
    let mut rx = store.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let FlowEvent::Updated { flow } = event
                && is_final(&flow)
            {
                println!("{}", line(&flow, false));
            }
        }
    });
    tokio::signal::ctrl_c().await?;
    handle.shutdown().await;
    Ok(())
}

async fn run_suite(home: PathBuf, args: RunArgs) -> Result<()> {
    use std::collections::BTreeMap;
    let text = std::fs::read_to_string(&args.suite).with_context(|| format!("reading {}", args.suite.display()))?;
    let is_postman = text.contains("\"info\"") && text.contains("\"item\"") && !text.contains("proxyzip-suite");
    let suite =
        if is_postman { proxyzip_suite::import_postman(&text)? } else { proxyzip_suite::Suite::from_json(&text)? };
    let mut variables = BTreeMap::new();
    for spec in &args.vars {
        let (k, v) = spec.split_once('=').with_context(|| format!("--var expects NAME=VALUE, got {spec:?}"))?;
        variables.insert(k.trim().to_string(), v.to_string());
    }
    let extra_root_pem = if args.trust_proxyzip_ca {
        Some(CertificateAuthority::load_or_create(&home)?.cert_pem().to_string())
    } else {
        None
    };
    let options = proxyzip_suite::RunOptions {
        environment: args.env.clone(),
        variables,
        proxy: args.via_proxy.clone(),
        extra_root_pem,
        insecure: args.insecure,
        timeout: std::time::Duration::from_secs(args.timeout),
        bail: args.bail,
        base_dir: args.suite.parent().map(PathBuf::from).unwrap_or_default(),
    };
    let report = proxyzip_suite::run(&suite, &options).await;
    if let Some(path) = &args.junit {
        std::fs::write(path, proxyzip_suite::to_junit(&report))?;
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.to_text());
    }
    if !report.ok() {
        std::process::exit(1);
    }
    Ok(())
}

fn validate(args: ValidateArgs) -> Result<()> {
    let text = std::fs::read_to_string(&args.spec).with_context(|| format!("reading {}", args.spec.display()))?;
    let spec = proxyzip_openapi::Spec::parse(&text)?;
    let flows = load_flows(&args.capture)?;
    let report = proxyzip_openapi::validate_flows(&spec, &flows);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}: {} request(s) checked, {} skipped", spec.title(), report.checked, report.skipped);
        for v in &report.violations {
            println!("✗ #{} {} {}", v.flow_id, v.method, v.url);
            println!("    {}{}", v.message, v.pointer.as_ref().map(|p| format!(" (at {p})")).unwrap_or_default());
        }
        if report.violations.is_empty() {
            println!("✓ everything conforms");
        } else {
            println!("{} problem(s) in {} request(s)", report.violations.len(), report.flows_with_violations().len());
        }
    }
    if !report.violations.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn code(args: CodeArgs) -> Result<()> {
    let language = proxyzip_export::codegen::Language::from_id(&args.lang).with_context(|| {
        let all: Vec<_> = proxyzip_export::codegen::Language::ALL.iter().map(|l| l.id()).collect();
        format!("unknown language {:?}; use one of {}", args.lang, all.join(", "))
    })?;
    let flows: Vec<_> = load_flows(&args.input)?.into_iter().filter(|f| f.request.method != "CONNECT").collect();
    let flow = args
        .number
        .checked_sub(1)
        .and_then(|i| flows.get(i))
        .with_context(|| format!("there is no request number {} (the file has {})", args.number, flows.len()))?;
    println!("{}", proxyzip_export::codegen::generate(flow, language));
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
    if let Some(path) = &args.workspace {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let workspace = proxyzip_export::workspace::read(&text)?;
        proxyzip_export::workspace::merge_rules(&mut rules, workspace.rules);
    }
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
            h3: args.reverse_h3,
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

    fn sample_session() -> (std::path::PathBuf, std::path::PathBuf) {
        use proxyzip_core::{Flow, Headers, Request, Scheme};
        let dir = std::env::temp_dir().join(format!(
            "proxyzip-cli-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let flow = Flow::new(
            1,
            "127.0.0.1:1".into(),
            Request {
                method: "GET".into(),
                scheme: Scheme::Https,
                host: "api.example.com".into(),
                port: 443,
                path: "/v1/items".into(),
                http_version: "HTTP/1.1".into(),
                headers: Headers(vec![("accept".into(), "application/json".into())]),
                body: Default::default(),
                body_truncated: false,
            },
        );
        let path = dir.join("demo.pzsession");
        std::fs::write(&path, proxyzip_export::session::write(&[flow]).unwrap()).unwrap();
        (dir, path)
    }

    #[test]
    fn export_converts_sessions() {
        let (dir, session) = sample_session();
        let csv = dir.join("out.csv");
        export(ExportArgs {
            input: session.clone(),
            format: ExportFormat::Csv,
            output: Some(csv.clone()),
            redact: false,
        })
        .unwrap();
        assert!(std::fs::read_to_string(&csv).unwrap().contains("https://api.example.com/v1/items"));
        let postman = dir.join("out.json");
        export(ExportArgs {
            input: session.clone(),
            format: ExportFormat::Postman,
            output: Some(postman.clone()),
            redact: false,
        })
        .unwrap();
        assert!(std::fs::read_to_string(&postman).unwrap().contains("collection/v2.1.0"));
        let har = dir.join("out.har");
        export(ExportArgs {
            input: session.clone(),
            format: ExportFormat::Har,
            output: Some(har.clone()),
            redact: false,
        })
        .unwrap();
        // The HAR we wrote can be loaded again.
        assert_eq!(load_flows(&har).unwrap().len(), 1);
        let err = export(ExportArgs { input: session, format: ExportFormat::Session, output: None, redact: false })
            .unwrap_err();
        assert!(err.to_string().contains("--output"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn code_validates_its_arguments() {
        let (dir, session) = sample_session();
        assert!(code(CodeArgs { input: session.clone(), lang: "python".into(), number: 1 }).is_ok());
        let unknown = code(CodeArgs { input: session.clone(), lang: "cobol".into(), number: 1 }).unwrap_err();
        assert!(unknown.to_string().contains("python"), "{unknown}");
        let missing = code(CodeArgs { input: session, lang: "go".into(), number: 5 }).unwrap_err();
        assert!(missing.to_string().contains("no request number 5"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
