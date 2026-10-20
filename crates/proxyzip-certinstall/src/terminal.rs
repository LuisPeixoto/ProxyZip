//! Open a terminal window whose environment already routes through ProxyZip.

use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};

use crate::runtime::{self, Shell};

/// Write the environment script for the platform's shell and open a new terminal that sources it.
pub fn open(host: &str, port: u16, cert_path: &Path) -> Result<()> {
    let files = runtime::prepare(cert_path)?;
    let dir = runtime::runtime_dir(cert_path);

    if cfg!(windows) {
        let script = dir.join("env.ps1");
        std::fs::write(&script, runtime::render(Shell::PowerShell, host, port, cert_path, &files))?;
        Command::new("cmd")
            .args(["/C", "start", "", "powershell", "-NoExit", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .spawn()?;
        return Ok(());
    }

    let env = dir.join("env.sh");
    std::fs::write(&env, runtime::render(Shell::Posix, host, port, cert_path, &files))?;
    let launcher = dir.join(if cfg!(target_os = "macos") { "terminal.command" } else { "terminal.sh" });
    std::fs::write(
        &launcher,
        format!(
            "#!/bin/sh\n. '{}'\necho 'This terminal sends its traffic through ProxyZip ({host}:{port}).'\nexec \"${{SHELL:-/bin/sh}}\" -i\n",
            env.display()
        ),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))?;
    }

    if cfg!(target_os = "macos") {
        Command::new("open").args(["-a", "Terminal"]).arg(&launcher).spawn()?;
        return Ok(());
    }
    let candidates: [(&str, &[&str]); 5] = [
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("alacritty", &["-e"]),
        ("xterm", &["-e"]),
    ];
    for (bin, args) in candidates {
        if Command::new(bin).args(args).arg(&launcher).spawn().is_ok() {
            return Ok(());
        }
    }
    bail!("no terminal emulator found; run `eval \"$(proxyzip env)\"` in your own terminal")
}
