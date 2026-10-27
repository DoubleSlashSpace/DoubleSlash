mod auth_prompt;
mod client;
mod embedded;
mod known_hosts;
mod openssh;
mod session_auth;
mod target;
mod traits;

pub use auth_prompt::{
    clear_session_password, env_key_suffix, env_name_for_host, password_from_env,
    per_host_user_from_env, SSH_PASSWORD_ENV, SSH_USER_ENV,
};
pub use client::*;
pub use embedded::EmbeddedTransport;
pub use openssh::OpenSshTransport;
pub use openssh::RemoteOutput;
pub use russh::keys::{PrivateKey, PublicKey};
pub use session_auth::{
    fetch_host_key, generate_ed25519_key, host_key_fingerprint, parse_private_key,
    parse_public_key, GeneratedKey, SessionAuth,
};
pub use target::SshTarget;
pub use traits::*;

pub use openssh::shell_escape;
