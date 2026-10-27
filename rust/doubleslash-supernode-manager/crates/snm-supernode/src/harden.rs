//! Baseline hardening for a server set up through the guided flow.
//!
//! This covers what can be automated safely on a fresh Debian or Ubuntu host:
//! a default-deny firewall that keeps SSH reachable, automatic security
//! updates, and optional fail2ban. It is a starting point, not ongoing
//! administration: the operator stays responsible for the server.
//!
//! Every step is idempotent, so a failed setup can be re-run from the top.

use anyhow::{bail, Context, Result};
use snm_core::PrivilegeMode;
use snm_transport::{shell_escape, SshTransport, Transport};

use crate::ops::{run_checked, upload_text_file};

/// What the probe learned about a host before anything is changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostOs {
    /// `/etc/os-release` `ID`, e.g. `debian` or `ubuntu`.
    pub id: String,
    pub id_like: String,
    pub version_id: String,
    pub pretty_name: String,
    pub systemd: bool,
    pub apt: bool,
    /// Logged in as uid 0.
    pub root: bool,
    /// `sudo -n true` succeeded.
    pub passwordless_sudo: bool,
    /// The port sshd accepted this session on, from `SSH_CONNECTION`. It can
    /// differ from the port dialled when a provider forwards SSH.
    pub ssh_port: Option<u16>,
    /// Sockets listening on a non-loopback address, as `proto address:port`.
    /// A default-deny firewall would cut these off.
    pub public_listeners: Vec<String>,
}

/// Whether the guided flow can set this host up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsSupport {
    Supported,
    Unsupported(String),
}

impl HostOs {
    pub fn support(&self) -> OsSupport {
        let family = format!("{} {}", self.id, self.id_like);
        if !family.contains("debian") && !family.contains("ubuntu") {
            return OsSupport::Unsupported(format!(
                "{} is not supported yet; use Debian 11+ or Ubuntu 20.04+",
                self.display_name()
            ));
        }
        if !self.systemd {
            return OsSupport::Unsupported("the server does not run systemd".into());
        }
        if !self.apt {
            return OsSupport::Unsupported("apt-get was not found on the server".into());
        }
        if let Some(reason) = self.too_old() {
            return OsSupport::Unsupported(reason);
        }
        if self.privilege().is_none() {
            return OsSupport::Unsupported(
                "log in as root, or as a user with passwordless sudo".into(),
            );
        }
        OsSupport::Supported
    }

    /// `Root` when logged in as root, `Sudo` when sudo needs no password.
    pub fn privilege(&self) -> Option<PrivilegeMode> {
        if self.root {
            Some(PrivilegeMode::Root)
        } else if self.passwordless_sudo {
            Some(PrivilegeMode::Sudo)
        } else {
            None
        }
    }

    pub fn display_name(&self) -> String {
        if self.pretty_name.is_empty() {
            format!("{} {}", self.id, self.version_id)
                .trim()
                .to_string()
        } else {
            self.pretty_name.clone()
        }
    }

    /// Older releases lack `sshd_config.d` and apt's lock timeout, which the
    /// steps below depend on.
    fn too_old(&self) -> Option<String> {
        let major: u32 = self
            .version_id
            .split('.')
            .next()
            .and_then(|m| m.parse().ok())?;
        let minimum = match self.id.as_str() {
            "debian" => 11,
            "ubuntu" => 20,
            _ => return None,
        };
        (major < minimum).then(|| format!("{} is too old for guided setup", self.display_name()))
    }
}

const PROBE_SCRIPT: &str = r#". /etc/os-release 2>/dev/null || true
echo "id=${ID:-}"
echo "id_like=${ID_LIKE:-}"
echo "version_id=${VERSION_ID:-}"
echo "pretty=${PRETTY_NAME:-}"
[ -d /run/systemd/system ] && echo "systemd=1"
command -v apt-get >/dev/null 2>&1 && echo "apt=1"
echo "uid=$(id -u)"
command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null && echo "sudo=1"
echo "ssh_connection=${SSH_CONNECTION:-}"
if command -v ss >/dev/null 2>&1; then ss -H -tuln 2>/dev/null | awk '{print "listen=" $1 " " $5}'; fi
"#;

pub async fn probe_host_os(transport: &SshTransport) -> Result<HostOs> {
    let output = run_checked(transport, &format!("sh -c {}", shell_escape(PROBE_SCRIPT)))
        .await
        .context("probe server operating system")?;
    Ok(parse_probe(&output.stdout))
}

fn parse_probe(stdout: &str) -> HostOs {
    let mut os = HostOs::default();
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key {
            "id" => os.id = value.to_ascii_lowercase(),
            "id_like" => os.id_like = value.to_ascii_lowercase(),
            "version_id" => os.version_id = value.to_string(),
            "pretty" => os.pretty_name = value.to_string(),
            "systemd" => os.systemd = value == "1",
            "apt" => os.apt = value == "1",
            "uid" => os.root = value == "0",
            "sudo" => os.passwordless_sudo = value == "1",
            "ssh_connection" => {
                os.ssh_port = value.split_whitespace().nth(3).and_then(|p| p.parse().ok());
            }
            "listen" => {
                if let Some(listener) = public_listener(value) {
                    if !os.public_listeners.contains(&listener) {
                        os.public_listeners.push(listener);
                    }
                }
            }
            _ => {}
        }
    }
    os
}

/// `tcp 0.0.0.0:80` stays; loopback and the stub resolver are dropped.
fn public_listener(line: &str) -> Option<String> {
    let (proto, address) = line.split_once(' ')?;
    let address = address.trim();
    let host = address.rsplit_once(':').map(|(h, _)| h).unwrap_or(address);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.split('%').next().unwrap_or(host);
    if host.starts_with("127.") || host == "::1" || host == "localhost" {
        return None;
    }
    Some(format!("{proto} {address}"))
}

/// Which hardening steps to run. The guided flow shows each one and lets the
/// user turn it off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardeningOptions {
    /// ufw: deny incoming by default, rate-limit SSH, then enable. The node's
    /// own ports are added by install.
    pub firewall: bool,
    /// unattended-upgrades for the distribution's security updates.
    pub automatic_security_updates: bool,
    /// Reboot at 04:00 server time when an update requires it. Briefly takes
    /// the node offline, but leaves no patched kernel waiting indefinitely.
    pub automatic_reboot: bool,
    /// Ban addresses that repeatedly fail SSH login.
    pub fail2ban: bool,
}

impl Default for HardeningOptions {
    fn default() -> Self {
        Self {
            firewall: true,
            automatic_security_updates: true,
            automatic_reboot: true,
            fail2ban: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardeningStep {
    InstallPackages(Vec<&'static str>),
    AutomaticUpdates { reboot: bool },
    Fail2ban,
    FirewallBaseline,
}

impl HardeningStep {
    pub fn title(&self) -> &'static str {
        match self {
            Self::InstallPackages(_) => "Install security packages",
            Self::AutomaticUpdates { .. } => "Turn on automatic security updates",
            Self::Fail2ban => "Ban repeated failed SSH logins",
            Self::FirewallBaseline => "Turn on the firewall",
        }
    }
}

/// Steps in the order they run. The firewall comes last among them so a
/// package install failure leaves the host as it was.
pub fn hardening_steps(options: &HardeningOptions) -> Vec<HardeningStep> {
    // curl is for the server-side updater; ca-certificates for its HTTPS.
    let mut packages = vec!["ca-certificates", "curl"];
    if options.firewall {
        packages.push("ufw");
    }
    if options.automatic_security_updates {
        packages.push("unattended-upgrades");
    }
    if options.fail2ban {
        // fail2ban reads the journal on Debian 12, which has no auth.log.
        packages.extend(["fail2ban", "python3-systemd"]);
    }
    let mut steps = vec![HardeningStep::InstallPackages(packages)];
    if options.automatic_security_updates {
        steps.push(HardeningStep::AutomaticUpdates {
            reboot: options.automatic_reboot,
        });
    }
    if options.fail2ban {
        steps.push(HardeningStep::Fail2ban);
    }
    if options.firewall {
        steps.push(HardeningStep::FirewallBaseline);
    }
    steps
}

pub async fn apply_hardening_step(
    transport: &SshTransport,
    privilege: PrivilegeMode,
    ssh_port: u16,
    step: &HardeningStep,
) -> Result<Vec<String>> {
    let prefix = crate::layout::privilege_prefix(privilege);
    match step {
        HardeningStep::InstallPackages(packages) => {
            run_root_script(transport, prefix, &install_packages_script(packages))
                .await
                .context("install packages")?;
            Ok(vec![format!("installed {}", packages.join(", "))])
        }
        HardeningStep::AutomaticUpdates { reboot } => {
            upload_text_file(
                transport,
                prefix,
                AUTO_UPGRADES_PATH,
                AUTO_UPGRADES_CONF,
                0o644,
            )
            .await?;
            upload_text_file(
                transport,
                prefix,
                UNATTENDED_REBOOT_PATH,
                &unattended_reboot_conf(*reboot),
                0o644,
            )
            .await?;
            run_root_script(
                transport,
                prefix,
                "systemctl enable --now apt-daily.timer apt-daily-upgrade.timer unattended-upgrades.service",
            )
            .await
            .context("enable automatic updates")?;
            Ok(vec![if *reboot {
                "automatic security updates on; reboots at 04:00 server time when required".into()
            } else {
                "automatic security updates on; reboots left to you".into()
            }])
        }
        HardeningStep::Fail2ban => {
            upload_text_file(
                transport,
                prefix,
                FAIL2BAN_JAIL_PATH,
                &fail2ban_jail(ssh_port),
                0o644,
            )
            .await?;
            run_root_script(
                transport,
                prefix,
                "systemctl enable fail2ban && systemctl restart fail2ban",
            )
            .await
            .context("start fail2ban")?;
            Ok(vec![format!("fail2ban watching SSH on port {ssh_port}")])
        }
        HardeningStep::FirewallBaseline => {
            let output = run_root_script(transport, prefix, &firewall_baseline_script(ssh_port))
                .await
                .context("enable firewall")?;
            let mut report = vec![format!(
                "firewall on: incoming denied except SSH (port {ssh_port}, rate-limited)"
            )];
            report.extend(output.lines().map(str::to_string));
            Ok(report)
        }
    }
}

async fn run_root_script(transport: &SshTransport, prefix: &str, script: &str) -> Result<String> {
    let command = format!("{prefix}sh -c {}", shell_escape(script));
    let output = transport.run(&command).await?;
    if output.exit_code != 0 {
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        bail!("exit {}: {detail}", output.exit_code);
    }
    Ok(output.stdout)
}

/// A fresh server often runs its first apt job at boot, so wait for the lock
/// rather than fail.
fn install_packages_script(packages: &[&str]) -> String {
    let list = packages
        .iter()
        .map(|p| shell_escape(p))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "set -e\nexport DEBIAN_FRONTEND=noninteractive\n\
         apt-get -q -o DPkg::Lock::Timeout=600 update\n\
         apt-get -q -y -o DPkg::Lock::Timeout=600 -o Dpkg::Options::=--force-confold \
         install --no-install-recommends {list}\n"
    )
}

const AUTO_UPGRADES_PATH: &str = "/etc/apt/apt.conf.d/20auto-upgrades";
const AUTO_UPGRADES_CONF: &str = "APT::Periodic::Update-Package-Lists \"1\";\n\
APT::Periodic::Unattended-Upgrade \"1\";\n";

const UNATTENDED_REBOOT_PATH: &str = "/etc/apt/apt.conf.d/52doubleslash-unattended-upgrades";

fn unattended_reboot_conf(reboot: bool) -> String {
    format!(
        "// Written by DoubleSlash supernode setup.\n\
         Unattended-Upgrade::Automatic-Reboot \"{reboot}\";\n\
         Unattended-Upgrade::Automatic-Reboot-Time \"04:00\";\n"
    )
}

const FAIL2BAN_JAIL_PATH: &str = "/etc/fail2ban/jail.d/doubleslash.local";

fn fail2ban_jail(ssh_port: u16) -> String {
    format!(
        "# Written by DoubleSlash supernode setup.\n\
         [sshd]\n\
         enabled = true\n\
         port = {ssh_port}\n\
         backend = systemd\n\
         maxretry = 5\n\
         findtime = 10m\n\
         bantime = 1h\n"
    )
}

/// SSH is allowed before the firewall is enabled, so the session running this
/// survives it.
fn firewall_baseline_script(ssh_port: u16) -> String {
    format!(
        "set -e\n\
         ufw default deny incoming\n\
         ufw default allow outgoing\n\
         ufw limit {ssh_port}/tcp comment 'snm:ssh'\n\
         ufw --force enable\n\
         ufw status | head -n 1\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEBIAN_PROBE: &str = "id=debian\nid_like=\nversion_id=12\npretty=Debian GNU/Linux 12 (bookworm)\n\
systemd=1\napt=1\nuid=0\nssh_connection=203.0.113.5 50122 198.51.100.7 22\n\
listen=udp 127.0.0.53%lo:53\nlisten=tcp 0.0.0.0:22\nlisten=tcp [::]:22\nlisten=tcp 127.0.0.1:631\nlisten=tcp [::1]:631\n";

    #[test]
    fn parses_a_debian_root_probe() {
        let os = parse_probe(DEBIAN_PROBE);
        assert_eq!(os.id, "debian");
        assert_eq!(os.version_id, "12");
        assert!(os.systemd && os.apt && os.root);
        assert_eq!(os.ssh_port, Some(22));
        assert_eq!(os.privilege(), Some(PrivilegeMode::Root));
        assert_eq!(os.support(), OsSupport::Supported);
        assert_eq!(os.public_listeners, vec!["tcp 0.0.0.0:22", "tcp [::]:22"]);
    }

    #[test]
    fn forwarded_ssh_reports_the_server_side_port() {
        let os = parse_probe("ssh_connection=203.0.113.5 50122 10.0.0.4 2222\n");
        assert_eq!(os.ssh_port, Some(2222));
    }

    #[test]
    fn ubuntu_derivative_with_sudo_is_supported() {
        let os = parse_probe(
            "id=pop\nid_like=\"ubuntu debian\"\nversion_id=22.04\nsystemd=1\napt=1\nuid=1000\nsudo=1\n",
        );
        assert_eq!(os.privilege(), Some(PrivilegeMode::Sudo));
        assert_eq!(os.support(), OsSupport::Supported);
    }

    #[test]
    fn refuses_unsupported_hosts_with_a_reason() {
        let fedora = parse_probe("id=fedora\nversion_id=40\nsystemd=1\nuid=0\n");
        assert!(matches!(fedora.support(), OsSupport::Unsupported(_)));
        let old = parse_probe("id=debian\nversion_id=10\nsystemd=1\napt=1\nuid=0\n");
        assert!(matches!(old.support(), OsSupport::Unsupported(r) if r.contains("too old")));
        let no_sudo = parse_probe("id=debian\nversion_id=12\nsystemd=1\napt=1\nuid=1000\n");
        assert!(matches!(no_sudo.support(), OsSupport::Unsupported(r) if r.contains("sudo")));
    }

    #[test]
    fn steps_follow_options_and_end_with_the_firewall() {
        let all = hardening_steps(&HardeningOptions::default());
        assert!(
            matches!(all.first(), Some(HardeningStep::InstallPackages(p)) if p.contains(&"fail2ban") && p.contains(&"ufw"))
        );
        assert_eq!(all.last(), Some(&HardeningStep::FirewallBaseline));

        let none = hardening_steps(&HardeningOptions {
            firewall: false,
            automatic_security_updates: false,
            automatic_reboot: false,
            fail2ban: false,
        });
        assert_eq!(
            none,
            vec![HardeningStep::InstallPackages(vec![
                "ca-certificates",
                "curl"
            ])]
        );
    }

    #[test]
    fn firewall_allows_ssh_before_enabling() {
        let script = firewall_baseline_script(2222);
        let allow = script.find("ufw limit 2222/tcp").unwrap();
        let enable = script.find("ufw --force enable").unwrap();
        assert!(allow < enable);
        assert!(script.contains("default deny incoming"));
    }

    #[test]
    fn package_install_waits_for_the_apt_lock() {
        let script = install_packages_script(&["ufw", "fail2ban"]);
        assert!(script.contains("DPkg::Lock::Timeout=600"));
        assert!(script.contains("DEBIAN_FRONTEND=noninteractive"));
        assert!(script.contains("install --no-install-recommends ufw fail2ban"));
    }

    #[test]
    fn reboot_setting_is_explicit_either_way() {
        assert!(unattended_reboot_conf(true).contains("Automatic-Reboot \"true\""));
        assert!(unattended_reboot_conf(false).contains("Automatic-Reboot \"false\""));
        assert!(fail2ban_jail(2222).contains("port = 2222"));
    }
}
