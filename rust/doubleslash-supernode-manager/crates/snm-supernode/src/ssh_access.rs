//! How the guided flow logs in, and how far it locks SSH down.
//!
//! The dangerous step is turning password login off: done before a key works,
//! it locks the owner out of their own server. So [`restrict_ssh_to_keys`]
//! takes a [`KeyLoginVerified`], which only [`verify_key_only_login`] can
//! produce, and only after a fresh connection logged in with the key alone.

use anyhow::{bail, Context, Result};
use snm_core::PrivilegeMode;
use snm_transport::{shell_escape, PrivateKey, PublicKey, SessionAuth, SshTransport, Transport};

use crate::ops::run_checked;

/// The three ways a user can choose to give the client SSH access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshAccessMode {
    /// Install a key generated for this client profile, confirm it logs in,
    /// then turn off password and keyboard-interactive login. Root keeps
    /// key-only login.
    ClientKeyOnly,
    /// Install the client key but leave sshd's settings alone. Password login
    /// stays on.
    ClientKeyAlongsidePassword,
    /// Use the user's own `~/.ssh` key. Nothing is installed or changed.
    OwnKey,
}

impl SshAccessMode {
    pub fn installs_client_key(self) -> bool {
        matches!(self, Self::ClientKeyOnly | Self::ClientKeyAlongsidePassword)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClientKeyOnly => "client-key-only",
            Self::ClientKeyAlongsidePassword => "client-key",
            Self::OwnKey => "own-key",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "client-key-only" => Some(Self::ClientKeyOnly),
            "client-key" => Some(Self::ClientKeyAlongsidePassword),
            "own-key" => Some(Self::OwnKey),
            _ => None,
        }
    }
}

/// Add `public_line` to the login user's `authorized_keys`, once.
pub async fn install_authorized_key(transport: &SshTransport, public_line: &str) -> Result<()> {
    let line = public_line.trim();
    if line.is_empty() || line.contains('\n') || line.contains('\r') {
        bail!("refusing to install a malformed public key");
    }
    run_checked(
        transport,
        &format!("sh -c {}", shell_escape(&authorized_key_script(line))),
    )
    .await
    .context("install SSH key")?;
    Ok(())
}

fn authorized_key_script(line: &str) -> String {
    let key = shell_escape(line);
    format!(
        "set -e\numask 077\n\
         f=\"$HOME/.ssh/authorized_keys\"\n\
         mkdir -p \"$HOME/.ssh\"\ntouch \"$f\"\n\
         chmod 700 \"$HOME/.ssh\"\nchmod 600 \"$f\"\n\
         if grep -qxF {key} \"$f\"; then exit 0; fi\n\
         if [ -s \"$f\" ] && [ -n \"$(tail -c1 \"$f\")\" ]; then echo >> \"$f\"; fi\n\
         printf '%s\\n' {key} >> \"$f\"\n"
    )
}

/// Proof that a key-only login to a host succeeded. Only
/// [`verify_key_only_login`] constructs it.
#[derive(Debug)]
pub struct KeyLoginVerified {
    target: String,
}

impl KeyLoginVerified {
    pub fn target(&self) -> &str {
        &self.target
    }
}

/// Open a new connection to `target` with `key` and no password at all, and
/// run a command over it.
pub async fn verify_key_only_login(
    target: &str,
    key: PrivateKey,
    host_key: PublicKey,
) -> Result<KeyLoginVerified> {
    let transport = SshTransport::with_auth(target, SessionAuth::pinned(host_key).with_key(key));
    let output = transport
        .run("true")
        .await
        .context("log in with the SSH key alone")?;
    if output.exit_code != 0 {
        bail!("key login to {target} ran, but a test command failed");
    }
    Ok(KeyLoginVerified {
        target: target.to_string(),
    })
}

const SSHD_DROPIN: &str = "/etc/ssh/sshd_config.d/10-doubleslash.conf";

/// Turn off password and keyboard-interactive login, and root password login.
///
/// The settings go in a drop-in that sorts ahead of cloud images' own
/// (`50-cloud-init.conf`), since sshd keeps the first value it reads. sshd
/// validates the result before reloading, and a rejected file is removed.
/// Existing sessions, including this one, stay open.
pub async fn restrict_ssh_to_keys(
    transport: &SshTransport,
    privilege: PrivilegeMode,
    proof: &KeyLoginVerified,
) -> Result<Vec<String>> {
    let prefix = crate::layout::privilege_prefix(privilege);
    let output = transport
        .run(&format!(
            "{prefix}sh -c {}",
            shell_escape(&sshd_lockdown_script())
        ))
        .await?;
    match output.exit_code {
        0 => Ok(vec![format!(
            "password login off for {}; key login only (remove {SSHD_DROPIN} to undo)",
            proof.target()
        )]),
        3 => bail!(
            "this server's sshd_config does not read {}; SSH settings left unchanged",
            "/etc/ssh/sshd_config.d"
        ),
        4 => bail!(
            "sshd rejected the new settings, so they were removed: {}",
            output.stderr.trim()
        ),
        5 => bail!(
            "the settings were applied, but another sshd setting still allows password login; \
             check /etc/ssh/sshd_config"
        ),
        code => bail!("restrict SSH login: exit {code}: {}", output.stderr.trim()),
    }
}

fn sshd_lockdown_script() -> String {
    format!(
        "set -u\n\
         conf={SSHD_DROPIN}\n\
         if ! grep -Eqi '^[[:space:]]*Include[[:space:]]+/etc/ssh/sshd_config\\.d/\\*\\.conf' /etc/ssh/sshd_config; then exit 3; fi\n\
         tmp=\"$conf.snm-new\"\n\
         {{\n\
         echo '# Written by DoubleSlash supernode setup. Delete this file to allow password login again.'\n\
         echo 'PasswordAuthentication no'\n\
         echo 'KbdInteractiveAuthentication no'\n\
         echo 'PermitRootLogin prohibit-password'\n\
         echo 'PermitEmptyPasswords no'\n\
         }} > \"$tmp\" || exit 6\n\
         chmod 644 \"$tmp\" && mv -f \"$tmp\" \"$conf\" || exit 6\n\
         if ! /usr/sbin/sshd -t; then rm -f \"$conf\"; exit 4; fi\n\
         systemctl try-reload-or-restart ssh 2>/dev/null || systemctl try-reload-or-restart sshd || exit 6\n\
         /usr/sbin/sshd -T 2>/dev/null | grep -qi '^passwordauthentication no' || exit 5\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_modes_round_trip() {
        for mode in [
            SshAccessMode::ClientKeyOnly,
            SshAccessMode::ClientKeyAlongsidePassword,
            SshAccessMode::OwnKey,
        ] {
            assert_eq!(SshAccessMode::parse(mode.as_str()), Some(mode));
        }
        assert!(!SshAccessMode::OwnKey.installs_client_key());
    }

    #[test]
    fn authorized_key_install_is_idempotent_and_escaped() {
        let script = authorized_key_script("ssh-ed25519 AAAA it's-me");
        let quoted = shell_escape("ssh-ed25519 AAAA it's-me");
        assert!(script.contains(&format!("grep -qxF {quoted}")));
        assert!(script.contains(&format!("printf '%s\\n' {quoted}")));
        let check = script.find("grep -qxF").unwrap();
        let append = script.find("printf").unwrap();
        assert!(check < append);
    }

    #[test]
    fn lockdown_validates_before_reloading_and_reverts_on_rejection() {
        let script = sshd_lockdown_script();
        let validate = script.find("/usr/sbin/sshd -t").unwrap();
        let reload = script.find("try-reload-or-restart").unwrap();
        assert!(validate < reload);
        assert!(script.contains("rm -f \"$conf\"; exit 4"));
        assert!(script.contains("PasswordAuthentication no"));
        assert!(script.contains("PermitRootLogin prohibit-password"));
        assert!(script.contains("10-doubleslash.conf"));
    }
}
