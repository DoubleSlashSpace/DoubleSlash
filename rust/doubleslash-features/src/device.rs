//! Device identifiers shared by credentials and transport routing.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

fn identity_key(identity: &str) -> String {
    let bare = identity.trim_end_matches('=');
    if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(bare) {
        if bytes.len() == 32 {
            return base64::engine::general_purpose::URL_SAFE.encode(bytes);
        }
    }
    identity.to_owned()
}

/// Public endpoint key bytes, distinct from the contact-facing identity key.
/// A routing identifier is not proof of authorization. The enclosing protocol
/// must authenticate the identity/device binding before using it as a route.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub [u8; 32]);

/// Live device routes per identity; historical registry tombstones are separate.
pub const MAX_LIVE_DEVICE_ROUTES: usize = 8;

/// Release gate shared by clients and relays while endpoint-aware room keys,
/// direct sessions and call arbitration are being integrated.
pub const DEVICE_ROUTING_READY: bool = cfg!(feature = "device-routing");

pub const DEVICE_WEBSOCKET_PROTOCOL: &str = "doubleslash.devices.v1";

#[derive(Debug, thiserror::Error)]
#[error("identity has reached its live device route limit")]
pub struct DeviceRouteLimit;

/// Endpoint-aware routing table. The `None` slot preserves a legacy connection;
/// device slots never overwrite it or one another. Authentication and quotas
/// remain the caller's responsibility, before registration and each send.
pub struct DeviceRoutes<T> {
    identities: HashMap<String, BTreeMap<Option<DeviceId>, T>>,
}

impl<T> Default for DeviceRoutes<T> {
    fn default() -> Self {
        Self {
            identities: HashMap::new(),
        }
    }
}

impl<T> DeviceRoutes<T> {
    /// Preserve the legacy map interface while transport consumers migrate.
    pub fn insert(&mut self, identity: String, sender: T) -> Option<T> {
        self.identities
            .entry(identity_key(&identity))
            .or_default()
            .insert(None, sender)
    }

    pub fn register_device(
        &mut self,
        identity: String,
        device: DeviceId,
        sender: T,
    ) -> Result<Option<T>, DeviceRouteLimit> {
        let routes = self.identities.entry(identity_key(&identity)).or_default();
        if !routes.contains_key(&Some(device))
            && routes.keys().filter(|key| key.is_some()).count() >= MAX_LIVE_DEVICE_ROUTES
        {
            return Err(DeviceRouteLimit);
        }
        Ok(routes.insert(Some(device), sender))
    }

    pub fn get(&self, identity: &str) -> Option<&T> {
        self.get_endpoint(identity, None)
    }

    pub fn get_endpoint(&self, identity: &str, device: Option<DeviceId>) -> Option<&T> {
        self.identities.get(&identity_key(identity))?.get(&device)
    }

    pub fn get_mut(&mut self, identity: &str) -> Option<&mut T> {
        self.get_endpoint_mut(identity, None)
    }

    pub fn get_endpoint_mut(&mut self, identity: &str, device: Option<DeviceId>) -> Option<&mut T> {
        self.identities
            .get_mut(&identity_key(identity))?
            .get_mut(&device)
    }

    /// Total live endpoints, including legacy slots, for transport capacity.
    pub fn len(&self) -> usize {
        self.identities.values().map(BTreeMap::len).sum()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, Option<DeviceId>, &T)> {
        self.identities.iter().flat_map(|(identity, routes)| {
            routes
                .iter()
                .map(move |(device, value)| (identity, *device, value))
        })
    }

    /// Only remove the caller's connection, even if a reconnect replaced it
    /// while the old connection was still finishing its read/write tasks.
    pub fn remove_endpoint_if(
        &mut self,
        identity: &str,
        device: Option<DeviceId>,
        matches: impl FnOnce(&T) -> bool,
    ) -> bool {
        let identity = identity_key(identity);
        let Some(routes) = self.identities.get_mut(&identity) else {
            return false;
        };
        if !routes.get(&device).is_some_and(matches) {
            return false;
        }
        routes.remove(&device);
        if routes.is_empty() {
            self.identities.remove(&identity);
        }
        true
    }

    pub fn remove(&mut self, identity: &str) -> Option<T> {
        let identity = identity_key(identity);
        let routes = self.identities.get_mut(&identity)?;
        let removed = routes.remove(&None);
        if routes.is_empty() {
            self.identities.remove(&identity);
        }
        removed
    }

    /// Whether any endpoint remains, for identity-level presence and teardown.
    pub fn contains_key(&self, identity: &str) -> bool {
        self.identities.contains_key(&identity_key(identity))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.identities.keys()
    }

    pub fn endpoints(&self, identity: &str) -> impl Iterator<Item = (Option<DeviceId>, &T)> {
        self.identities
            .get(&identity_key(identity))
            .into_iter()
            .flat_map(|routes| routes.iter().map(|(id, sender)| (*id, sender)))
    }

    pub fn is_empty(&self) -> bool {
        self.identities.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_and_contact_identities_are_isolated() {
        let mut routes = DeviceRoutes::default();
        routes.insert("alice".into(), "legacy");
        routes
            .register_device("alice".into(), DeviceId([1; 32]), "desktop")
            .unwrap();
        routes
            .register_device("alice".into(), DeviceId([2; 32]), "phone")
            .unwrap();
        routes
            .register_device("bob".into(), DeviceId([1; 32]), "other identity")
            .unwrap();
        assert_eq!(routes.get("alice"), Some(&"legacy"));
        assert_eq!(
            routes.get_endpoint("alice", Some(DeviceId([1; 32]))),
            Some(&"desktop")
        );
        assert_eq!(
            routes.get_endpoint("alice", Some(DeviceId([2; 32]))),
            Some(&"phone")
        );
        assert_eq!(
            routes.get_endpoint("bob", Some(DeviceId([1; 32]))),
            Some(&"other identity")
        );
        assert!(routes
            .get_endpoint("alice", Some(DeviceId([3; 32])))
            .is_none());
    }

    #[test]
    fn reconnect_cleanup_cannot_remove_replacement_or_another_device() {
        let mut routes = DeviceRoutes::default();
        let phone = DeviceId([1; 32]);
        let desktop = DeviceId([2; 32]);
        routes.register_device("alice".into(), phone, 1).unwrap();
        routes.register_device("alice".into(), desktop, 2).unwrap();
        assert_eq!(
            routes.register_device("alice".into(), phone, 3).unwrap(),
            Some(1)
        );
        assert!(!routes.remove_endpoint_if("alice", Some(phone), |value| *value == 1));
        assert!(routes.remove_endpoint_if("alice", Some(phone), |value| *value == 3));
        assert!(routes.contains_key("alice"));
        assert_eq!(routes.get_endpoint("alice", Some(desktop)), Some(&2));
        assert!(routes.remove_endpoint_if("alice", Some(desktop), |_| true));
        assert!(routes.is_empty());
    }

    #[test]
    fn route_limit_bounds_new_devices_but_allows_reconnection() {
        let mut routes = DeviceRoutes::default();
        for i in 0..MAX_LIVE_DEVICE_ROUTES {
            routes
                .register_device("alice".into(), DeviceId([i as u8; 32]), i)
                .unwrap();
        }
        assert!(routes
            .register_device("alice".into(), DeviceId([99; 32]), 99)
            .is_err());
        assert_eq!(
            routes
                .register_device("alice".into(), DeviceId([0; 32]), 42)
                .unwrap(),
            Some(0)
        );
        assert_eq!(routes.endpoints("alice").count(), MAX_LIVE_DEVICE_ROUTES);
        routes.insert("alice".into(), 50);
        routes.remove("alice");
        assert_eq!(routes.endpoints("alice").count(), MAX_LIVE_DEVICE_ROUTES);
    }

    #[test]
    fn identity_padding_cannot_split_presence_or_bypass_route_limit() {
        let bare = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 32]);
        let padded = format!("{bare}=");
        let mut routes = DeviceRoutes::default();
        for i in 0..MAX_LIVE_DEVICE_ROUTES {
            let identity = if i % 2 == 0 { &bare } else { &padded };
            routes
                .register_device(identity.clone(), DeviceId([i as u8; 32]), i)
                .unwrap();
        }
        assert_eq!(routes.keys().count(), 1);
        assert!(routes
            .register_device(bare.clone(), DeviceId([99; 32]), 99)
            .is_err());
        assert!(routes
            .register_device(padded.clone(), DeviceId([99; 32]), 99)
            .is_err());
        assert_eq!(
            routes.get_endpoint(&bare, Some(DeviceId([0; 32]))),
            Some(&0)
        );
        assert!(routes.remove_endpoint_if(&padded, Some(DeviceId([0; 32])), |value| *value == 0));
    }
}
