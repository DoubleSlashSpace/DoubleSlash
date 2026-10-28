//! Supernode binary updates with rollback, run on the server itself.
//!
//! One script serves both update modes the guided flow offers. A systemd timer
//! runs it daily in [`UpdateMode::ServerTimer`]; in
//! [`UpdateMode::ClientDriven`] the timer is off and it runs only when the
//! client asks. Either way it downloads the release package, checks it against
//! the published SHA-256 sidecar, installs it beside the running binary, flips
//! the `current` link, restarts every instance on the host, and flips back if
//! any instance fails to stay up.
//!
//! Only the binary changes: `identity.json`, peers, and every other file in a
//! data directory are left alone.

use anyhow::{bail, Context, Result};
use snm_core::Defaults;
use snm_transport::{shell_escape, SshTransport, Transport};

use crate::layout::privilege_prefix;
use crate::ops::{run_checked, upload_text_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    /// A daily timer on the server keeps the node current, client or not.
    ServerTimer,
    /// Updates run only when the client asks.
    ClientDriven,
}

impl UpdateMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServerTimer => "server-timer",
            Self::ClientDriven => "client",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "server-timer" => Some(Self::ServerTimer),
            "client" => Some(Self::ClientDriven),
            _ => None,
        }
    }
}

pub const UPDATER_SCRIPT_PATH: &str = "/usr/local/lib/doubleslash/update-supernode";
pub const UPDATER_ENV_PATH: &str = "/etc/doubleslash/update.env";
pub const UPDATER_UNIT: &str = "doubleslash-supernode-update.service";
pub const UPDATER_TIMER: &str = "doubleslash-supernode-update.timer";
const UPDATER_STATE_DIR: &str = "/var/lib/doubleslash-update";

/// Where the updater fetches from and installs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterConfig {
    /// GitHub `owner/repo`.
    pub repo: String,
    /// Release channel or version: `nightly`, or a tag such as `1.2.0`.
    pub channel: String,
    /// `linux-x86_64` or `linux-aarch64`.
    pub platform: String,
    pub install_root: String,
}

impl UpdaterConfig {
    pub fn from_defaults(defaults: &Defaults, platform: &str) -> Result<Self> {
        if defaults.version == "local" {
            bail!("a locally built node cannot update itself from a release");
        }
        let config = Self {
            repo: defaults.release_repo.clone(),
            channel: defaults.version.clone(),
            platform: platform.to_string(),
            install_root: defaults.install_root.clone(),
        };
        config.validate()?;
        Ok(config)
    }

    /// The env file is sourced by a root shell, so every value must be inert.
    fn validate(&self) -> Result<()> {
        let safe = |value: &str, extra: &str| {
            !value.is_empty()
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c) || extra.contains(c))
        };
        if !safe(&self.repo, "/") || self.repo.matches('/').count() != 1 {
            bail!(
                "release repository must look like owner/name: {:?}",
                self.repo
            );
        }
        if !safe(&self.channel, "") {
            bail!("unsupported release channel {:?}", self.channel);
        }
        if !matches!(self.platform.as_str(), "linux-x86_64" | "linux-aarch64") {
            bail!("no server-side updates for platform {:?}", self.platform);
        }
        if !self.install_root.starts_with('/') || !safe(&self.install_root, "/") {
            bail!(
                "install root must be an absolute plain path: {:?}",
                self.install_root
            );
        }
        Ok(())
    }

    fn release_tag(&self) -> String {
        match self.channel.as_str() {
            "nightly" | "latest-nightly" | "latest" => "nightly".into(),
            v if v.starts_with('v') => v.into(),
            v => format!("v{v}"),
        }
    }

    fn asset_version(&self) -> &str {
        match self.channel.as_str() {
            "nightly" | "latest-nightly" | "latest" => "nightly",
            v => v.trim_start_matches('v'),
        }
    }

    pub fn render_env(&self) -> String {
        let asset = format!(
            "doubleslash-supernode-{}-{}.tar.gz",
            self.asset_version(),
            self.platform
        );
        format!(
            "# Written by DoubleSlash supernode setup.\n\
             RELEASE_URL=https://github.com/{repo}/releases/download/{tag}/{asset}\n\
             CHANNEL={channel}\n\
             INSTALL_ROOT={root}\n\
             STATE_DIR={UPDATER_STATE_DIR}\n",
            repo = self.repo,
            tag = self.release_tag(),
            channel = self.asset_version(),
            root = self.install_root,
        )
    }
}

/// Usage: `update-supernode` fetches and installs the latest package;
/// `update-supernode check` prints `update-available` or `up-to-date`;
/// `update-supernode local <archive> <sha256>` installs an archive the client
/// downloaded, verified and uploaded (for servers that cannot reach GitHub).
pub const UPDATER_SCRIPT: &str = r#"#!/bin/sh
# Written by DoubleSlash supernode setup. Updates the supernode binary and
# rolls back if an instance does not stay up.
# Usage: update-supernode [check | local <archive> <sha256>]
set -eu
. /etc/doubleslash/update.env
mode="${1:-install}"

# A timer run and an update pushed from DoubleSlash must not overlap.
exec 9>/run/doubleslash-update.lock
flock -n 9 || { echo "another supernode update is already running" >&2; exit 6; }

fetch() {
  curl -fsSL --proto '=https' --retry 3 --max-time 600 -o "$2" "$1"
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [ "$mode" = local ]; then
  [ -f "${2:-}" ] || { echo "no package to install" >&2; exit 2; }
  expected="${3:-}"
else
  fetch "$RELEASE_URL.sha256" "$work/package.sha256"
  expected=$(awk 'NR == 1 { print $1 }' "$work/package.sha256")
fi
case "$expected" in
  *[!0-9a-f]*|"") echo "the published checksum is malformed" >&2; exit 2 ;;
esac
[ "${#expected}" -eq 64 ] || { echo "the published checksum is malformed" >&2; exit 2; }

installed=""
[ -f "$STATE_DIR/package.sha256" ] && installed=$(cat "$STATE_DIR/package.sha256")
if [ "$mode" = check ]; then
  if [ "$installed" = "$expected" ]; then echo up-to-date; else echo update-available; fi
  exit 0
fi
if [ "$installed" = "$expected" ] && [ -x "$INSTALL_ROOT/bin/current" ]; then
  echo "already running the latest $CHANNEL package"
  exit 0
fi

if [ "$mode" = local ]; then
  cp "$2" "$work/package.tar.gz"
else
  fetch "$RELEASE_URL" "$work/package.tar.gz"
fi
actual=$(sha256sum "$work/package.tar.gz" | awk '{ print $1 }')
if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch: published $expected, downloaded $actual" >&2
  exit 3
fi
mkdir "$work/x"
tar -xzf "$work/package.tar.gz" -C "$work/x" --no-same-owner --no-same-permissions
new_bin=$(find "$work/x" -mindepth 2 -maxdepth 2 -type f -name doubleslash-supernode | head -n 1)
[ -n "$new_bin" ] || { echo "the package has no doubleslash-supernode binary" >&2; exit 4; }
notices="$(dirname "$new_bin")/licenses"
for notice in rust-licenses.html DoubleSlash-LICENSE.txt inventory.json; do
  [ -s "$notices/$notice" ] || { echo "the package is missing $notice" >&2; exit 4; }
done

short=$(printf '%s' "$expected" | cut -c1-12)
target="$INSTALL_ROOT/bin/doubleslash-supernode-$CHANNEL-$short"
install -d -m 0755 "$INSTALL_ROOT/bin"
rm -rf "$target.licenses.new"
cp -R "$notices" "$target.licenses.new"
chmod -R a+rX,go-w "$target.licenses.new"
rm -rf "$target.licenses"
mv "$target.licenses.new" "$target.licenses"
install -m 0755 "$new_bin" "$target.new"
mv -f "$target.new" "$target"

previous=$(readlink "$INSTALL_ROOT/bin/current" 2>/dev/null || true)
units=$(systemctl list-units --state=active --plain --no-legend 'doubleslash-supernode@*.service' | awk '{ print $1 }')

restart_all() {
  for unit in $units; do systemctl restart "$unit" || true; done
}

# Up after 3 s, and still the same process 20 s later: a crash loop under
# Restart=on-failure shows up as a new start time.
healthy() {
  sleep 3
  for unit in $units; do
    printf '%s %s\n' "$unit" "$(systemctl show -p ExecMainStartTimestampMonotonic --value "$unit")"
  done > "$work/started"
  sleep 20
  while read -r unit started; do
    systemctl is-active --quiet "$unit" || return 1
    [ "$(systemctl show -p ExecMainStartTimestampMonotonic --value "$unit")" = "$started" ] || return 1
  done < "$work/started"
  return 0
}

ln -sfn "$target" "$INSTALL_ROOT/bin/current"
restart_all
if [ -n "$units" ] && ! healthy; then
  echo "the new binary did not stay up; rolling back" >&2
  if [ -n "$previous" ]; then
    ln -sfn "$previous" "$INSTALL_ROOT/bin/current"
    restart_all
    [ "$previous" != "$target" ] && rm -rf "$target" "$target.licenses"
  fi
  exit 5
fi

install -d -m 0755 "$STATE_DIR"
[ -n "$previous" ] && [ "$previous" != "$target" ] && printf '%s\n' "$previous" > "$STATE_DIR/previous"
printf '%s' "$expected" > "$STATE_DIR/package.sha256"

# Keep the running binary and the one before it; drop older updater builds.
keep_prev=$(cat "$STATE_DIR/previous" 2>/dev/null || true)
for old in "$INSTALL_ROOT"/bin/doubleslash-supernode-*-????????????; do
  [ -f "$old" ] || continue
  [ "$old" = "$target" ] && continue
  [ "$old" = "$keep_prev" ] && continue
  rm -rf "$old" "$old.licenses"
done
echo "updated to $CHANNEL package $short"
"#;

fn updater_service() -> String {
    format!(
        "[Unit]\n\
         Description=Update the DoubleSlash supernode\n\
         Wants=network-online.target\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={UPDATER_SCRIPT_PATH}\n\
         TimeoutStartSec=30min\n"
    )
}

fn updater_timer() -> String {
    "[Unit]\n\
     Description=Daily DoubleSlash supernode update\n\
     \n\
     [Timer]\n\
     OnCalendar=daily\n\
     RandomizedDelaySec=2h\n\
     Persistent=true\n\
     \n\
     [Install]\n\
     WantedBy=timers.target\n"
        .to_string()
}

/// Install the script, its config and units; enable the timer only in
/// [`UpdateMode::ServerTimer`].
pub async fn install_updater(
    transport: &SshTransport,
    privilege: snm_core::PrivilegeMode,
    config: &UpdaterConfig,
    mode: UpdateMode,
) -> Result<Vec<String>> {
    config.validate()?;
    let prefix = privilege_prefix(privilege);
    upload_text_file(
        transport,
        prefix,
        UPDATER_ENV_PATH,
        &config.render_env(),
        0o644,
    )
    .await?;
    upload_text_file(
        transport,
        prefix,
        UPDATER_SCRIPT_PATH,
        UPDATER_SCRIPT,
        0o755,
    )
    .await?;
    upload_text_file(
        transport,
        prefix,
        &format!("/etc/systemd/system/{UPDATER_UNIT}"),
        &updater_service(),
        0o644,
    )
    .await?;
    upload_text_file(
        transport,
        prefix,
        &format!("/etc/systemd/system/{UPDATER_TIMER}"),
        &updater_timer(),
        0o644,
    )
    .await?;
    run_checked(transport, &format!("{prefix}systemctl daemon-reload")).await?;
    set_update_mode(transport, privilege, mode).await
}

pub async fn set_update_mode(
    transport: &SshTransport,
    privilege: snm_core::PrivilegeMode,
    mode: UpdateMode,
) -> Result<Vec<String>> {
    let prefix = privilege_prefix(privilege);
    match mode {
        UpdateMode::ServerTimer => {
            run_checked(
                transport,
                &format!("{prefix}systemctl enable --now {UPDATER_TIMER}"),
            )
            .await?;
            Ok(vec![
                "automatic supernode updates on: checked daily, rolled back if the node fails to start"
                    .into(),
            ])
        }
        UpdateMode::ClientDriven => {
            run_checked(
                transport,
                &format!("{prefix}systemctl disable --now {UPDATER_TIMER} 2>/dev/null || true"),
            )
            .await?;
            Ok(vec![
                "supernode updates run when you choose Update in DoubleSlash".into(),
            ])
        }
    }
}

/// Record the package a fresh install came from, so the updater's first run
/// does not reinstall the same build.
pub async fn record_installed_package(
    transport: &SshTransport,
    privilege: snm_core::PrivilegeMode,
    archive_sha256: &str,
) -> Result<()> {
    if archive_sha256.len() != 64 || !archive_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("not a SHA-256 digest: {archive_sha256:?}");
    }
    let prefix = privilege_prefix(privilege);
    upload_text_file(
        transport,
        prefix,
        &format!("{UPDATER_STATE_DIR}/package.sha256"),
        &archive_sha256.to_ascii_lowercase(),
        0o644,
    )
    .await
}

/// The SHA-256 of the package the updater (or setup) last installed, if any.
pub async fn installed_package(
    transport: &SshTransport,
    privilege: snm_core::PrivilegeMode,
) -> Result<Option<String>> {
    let prefix = privilege_prefix(privilege);
    let output = run_checked(
        transport,
        &format!("{prefix}cat {UPDATER_STATE_DIR}/package.sha256 2>/dev/null || true"),
    )
    .await?;
    let digest = output.stdout.trim().to_ascii_lowercase();
    Ok((digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit())).then_some(digest))
}

/// Install a package the caller downloaded and verified: upload it, then run
/// the updater's `local` mode, which checks the digest again and rolls back
/// like a timer run. Works on servers that cannot reach GitHub themselves.
pub async fn push_update(
    transport: &SshTransport,
    privilege: snm_core::PrivilegeMode,
    archive: &[u8],
    archive_sha256: &str,
) -> Result<UpdateOutcome> {
    if archive_sha256.len() != 64 || !archive_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("not a SHA-256 digest: {archive_sha256:?}");
    }
    let prefix = privilege_prefix(privilege);
    let staged = format!("/tmp/doubleslash-update-{}.tar.gz", &archive_sha256[..12]);
    transport
        .upload_bytes(&staged, archive, 0o600)
        .await
        .context("upload the supernode package")?;
    let run = transport
        .run(&format!(
            "{prefix}{UPDATER_SCRIPT_PATH} local {staged} {archive_sha256}"
        ))
        .await;
    let _ = transport.run(&format!("rm -f {staged}")).await;
    let output = run?;
    let log = output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .map(str::to_string)
        .collect();
    Ok(UpdateOutcome {
        succeeded: output.exit_code == 0,
        log,
    })
}

/// Whether a newer package than the one the updater last installed is out.
/// A node installed without the updater reports available until its first
/// update.
pub async fn check_for_update(
    transport: &SshTransport,
    privilege: snm_core::PrivilegeMode,
) -> Result<bool> {
    let prefix = privilege_prefix(privilege);
    let output = run_checked(transport, &format!("{prefix}{UPDATER_SCRIPT_PATH} check"))
        .await
        .context("check for a supernode update")?;
    match output.stdout.trim() {
        "update-available" => Ok(true),
        "up-to-date" => Ok(false),
        other => bail!("unexpected update check output: {other}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOutcome {
    pub succeeded: bool,
    /// The updater's own log lines from this run.
    pub log: Vec<String>,
}

/// Run the updater now through its unit, so a run started here and one
/// started by the timer can never overlap. Returns when it has finished.
pub async fn run_update_now(
    transport: &SshTransport,
    privilege: snm_core::PrivilegeMode,
) -> Result<UpdateOutcome> {
    let prefix = privilege_prefix(privilege);
    let cursor = run_checked(
        transport,
        &format!("{prefix}journalctl -u {UPDATER_UNIT} -n 0 --show-cursor --no-pager"),
    )
    .await
    .ok()
    .and_then(|o| {
        o.stdout
            .lines()
            .find_map(|l| l.strip_prefix("-- cursor: ").map(str::to_string))
    });
    let started = transport
        .run(&format!("{prefix}systemctl start {UPDATER_UNIT}"))
        .await?;
    let since = cursor
        .map(|c| format!("--after-cursor={}", shell_escape(&c)))
        .unwrap_or_else(|| "-n 50".into());
    let log = run_checked(
        transport,
        &format!("{prefix}journalctl -u {UPDATER_UNIT} {since} -o cat --no-pager"),
    )
    .await
    .map(|o| o.stdout.lines().map(str::to_string).collect())
    .unwrap_or_default();
    Ok(UpdateOutcome {
        succeeded: started.exit_code == 0,
        log,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> UpdaterConfig {
        UpdaterConfig {
            repo: "DoubleSlashSpace/DoubleSlash".into(),
            channel: "nightly".into(),
            platform: "linux-x86_64".into(),
            install_root: "/opt/doubleslash".into(),
        }
    }

    #[test]
    fn env_points_at_the_same_asset_as_install() {
        let env = config().render_env();
        assert!(env.contains(
            "RELEASE_URL=https://github.com/DoubleSlashSpace/DoubleSlash/releases/download/nightly/doubleslash-supernode-nightly-linux-x86_64.tar.gz\n"
        ));
        let defaults = Defaults {
            version: "nightly".into(),
            ..Defaults::default()
        };
        let artifact = crate::release::resolve_release_artifact(&defaults, "linux-x86_64").unwrap();
        assert!(env.contains(&artifact.asset_url));
    }

    #[test]
    fn tagged_channel_uses_v_tag_and_bare_asset_version() {
        let env = UpdaterConfig {
            channel: "1.2.0".into(),
            ..config()
        }
        .render_env();
        assert!(env.contains("/download/v1.2.0/doubleslash-supernode-1.2.0-linux-x86_64.tar.gz"));
    }

    #[test]
    fn refuses_values_a_root_shell_would_interpret() {
        for bad in [
            UpdaterConfig {
                repo: "a/b; rm -rf /".into(),
                ..config()
            },
            UpdaterConfig {
                channel: "$(id)".into(),
                ..config()
            },
            UpdaterConfig {
                install_root: "opt/doubleslash".into(),
                ..config()
            },
            UpdaterConfig {
                platform: "win64".into(),
                ..config()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        assert!(config().validate().is_ok());
    }

    #[test]
    fn local_builds_cannot_self_update() {
        let defaults = Defaults {
            version: "local".into(),
            ..Defaults::default()
        };
        assert!(UpdaterConfig::from_defaults(&defaults, "linux-x86_64").is_err());
    }

    #[test]
    fn script_verifies_before_installing_and_rolls_back() {
        let s = UPDATER_SCRIPT;
        let verify = s.find("checksum mismatch").unwrap();
        let link = s.find("ln -sfn \"$target\"").unwrap();
        assert!(verify < link);
        assert!(s.contains("--proto '=https'"));
        assert!(s.contains("ln -sfn \"$previous\""));
        assert!(s.contains("rolling back"));
        // A pushed package is checked against the digest the client passed,
        // by the same comparison a fetched one is.
        assert!(s.contains("expected=\"${3:-}\""));
        assert!(s.contains("flock -n 9"));
    }

    #[test]
    fn modes_round_trip() {
        for mode in [UpdateMode::ServerTimer, UpdateMode::ClientDriven] {
            assert_eq!(UpdateMode::parse(mode.as_str()), Some(mode));
        }
        assert!(updater_timer().contains("Persistent=true"));
        assert!(updater_service().contains("Type=oneshot"));
    }
}
