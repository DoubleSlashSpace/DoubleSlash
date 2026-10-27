//! Host and manage a supernode from the client.
//!
//! Drives the supernode manager's library (`snm-supernode`) over SSH: set up a
//! server in a few steps, harden it, keep it updated, cluster further servers
//! into it, and connect to it through its signed invite. The supernode stays
//! an ordinary transport peer: nothing here grants it identity authority or
//! bypasses invite acceptance.
//!
//! Desktop only (`supernode-admin`, turned on by `qt-ui`). See
//! `docs/SUPERNODE_CLIENT_ADMIN_PLAN.md`.

pub mod service;
pub mod store;

pub use service::{AdminService, Request, RESPONSIBILITY_NOTICE};
pub use store::{ManagedHost, ManagedNode, ManagedNodeStore};
