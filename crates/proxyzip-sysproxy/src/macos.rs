use anyhow::{Result, bail};

use crate::{DEFAULT_BYPASS, SystemProxyStatus, run};

fn services() -> Result<Vec<String>> {
    let out = run("networksetup", &["-listallnetworkservices"])?;
    // First line is an explanatory banner; disabled services start with '*'.
    Ok(out.lines().skip(1).filter(|l| !l.is_empty() && !l.starts_with('*')).map(str::to_string).collect())
}

fn commands(host: &str, port: u16, on: bool) -> Result<Vec<Vec<String>>> {
    let port = port.to_string();
    let mut cmds = Vec::new();
    for svc in services()? {
        if on {
            cmds.push(vec!["-setwebproxy".into(), svc.clone(), host.into(), port.clone()]);
            cmds.push(vec!["-setsecurewebproxy".into(), svc.clone(), host.into(), port.clone()]);
            let mut bypass = vec!["-setproxybypassdomains".into(), svc.clone()];
            bypass.extend(DEFAULT_BYPASS.iter().map(|s| s.to_string()));
            cmds.push(bypass);
        }
        let state = if on { "on" } else { "off" };
        cmds.push(vec!["-setwebproxystate".into(), svc.clone(), state.into()]);
        cmds.push(vec!["-setsecurewebproxystate".into(), svc, state.into()]);
    }
    Ok(cmds)
}

fn apply(cmds: Vec<Vec<String>>) -> Result<()> {
    let direct = cmds.iter().try_for_each(|c| {
        let args: Vec<&str> = c.iter().map(String::as_str).collect();
        run("networksetup", &args).map(|_| ())
    });
    if direct.is_ok() {
        return Ok(());
    }
    // Not permitted for this user: ask for administrator rights once for the whole batch.
    let script = cmds
        .iter()
        .map(|c| {
            let quoted: Vec<String> = c.iter().map(|a| format!("'{}'", a.replace('\'', r"'\''"))).collect();
            format!("/usr/sbin/networksetup {}", quoted.join(" "))
        })
        .collect::<Vec<_>>()
        .join(" && ");
    let apple = format!(
        "do shell script \"{}\" with administrator privileges with prompt \"ProxyZip wants to change your proxy settings.\"",
        script.replace('\\', "\\\\").replace('"', "\\\"")
    );
    run("osascript", &["-e", &apple]).map(|_| ())
}

pub fn enable(host: &str, port: u16) -> Result<()> {
    apply(commands(host, port, true)?)
}

pub fn disable() -> Result<()> {
    apply(commands("", 0, false)?)
}

pub fn status() -> Result<SystemProxyStatus> {
    // `scutil --proxy` reports the effective, merged configuration.
    let out = run("scutil", &["--proxy"])?;
    let get = |key: &str| {
        out.lines().find_map(|l| {
            let (k, v) = l.trim().split_once(" : ")?;
            (k == key).then(|| v.trim().to_string())
        })
    };
    let enabled = get("HTTPSEnable").as_deref() == Some("1") || get("HTTPEnable").as_deref() == Some("1");
    let host = get("HTTPSProxy").or_else(|| get("HTTPProxy"));
    let port = get("HTTPSPort").or_else(|| get("HTTPPort")).and_then(|p| p.parse().ok());
    if out.is_empty() {
        bail!("scutil returned no proxy information");
    }
    Ok(SystemProxyStatus { enabled, host, port })
}
