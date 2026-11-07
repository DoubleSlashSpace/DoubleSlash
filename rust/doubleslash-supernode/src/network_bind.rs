//! Explicit dual-stack listeners for IPv6 wildcard binds.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};

use socket2::{Domain, Protocol, Socket, Type};

fn ipv6_available() -> bool {
    Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))
        .and_then(|socket| socket.bind(&SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 0).into()))
        .is_ok()
}

fn bind_with_fallback<T>(
    address: SocketAddr,
    mut bind: impl FnMut(SocketAddr) -> io::Result<T>,
    ipv6_available: impl FnOnce() -> bool,
) -> io::Result<T> {
    match bind(address) {
        Err(error)
            if address.ip() == Ipv6Addr::UNSPECIFIED
                && !matches!(
                    error.kind(),
                    io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                )
                && !ipv6_available() =>
        {
            let fallback = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), address.port());
            tracing::warn!(%address, %fallback, %error, "IPv6 unavailable; binding IPv4 instead");
            bind(fallback)
        }
        result => result,
    }
}

fn bind_socket(address: SocketAddr, kind: Type, protocol: Protocol) -> io::Result<Socket> {
    bind_with_fallback(
        address,
        |address| {
            let socket = Socket::new(Domain::for_address(address), kind, Some(protocol))?;
            if address.ip() == Ipv6Addr::UNSPECIFIED {
                socket.set_only_v6(false)?;
            }
            #[cfg(unix)]
            if kind == Type::STREAM {
                socket.set_reuse_address(true)?;
            }
            socket.set_nonblocking(true)?;
            socket.bind(&address.into())?;
            Ok(socket)
        },
        ipv6_available,
    )
}

pub(crate) fn tcp_listener(address: SocketAddr) -> io::Result<tokio::net::TcpListener> {
    let socket = bind_socket(address, Type::STREAM, Protocol::TCP)?;
    socket.listen(1024)?;
    tokio::net::TcpListener::from_std(socket.into())
}

pub(crate) fn udp_socket(address: SocketAddr) -> io::Result<UdpSocket> {
    Ok(bind_socket(address, Type::DGRAM, Protocol::UDP)?.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn wildcard_tcp_accepts_ipv4_and_ipv6_on_one_port() -> io::Result<()> {
        let listener = tcp_listener(SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0))?;
        assert!(
            listener.local_addr()?.is_ipv6(),
            "this test requires local IPv6"
        );
        let port = listener.local_addr()?.port();
        for ip in [Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()] {
            let client = tokio::net::TcpStream::connect(SocketAddr::new(ip, port)).await?;
            let (_, peer) =
                tokio::time::timeout(Duration::from_secs(2), listener.accept()).await??;
            assert!(peer.ip().is_loopback() || peer.ip().to_canonical().is_loopback());
            drop(client);
        }
        Ok(())
    }

    #[tokio::test]
    async fn wildcard_udp_receives_both_families_and_reopens_the_same_port() -> io::Result<()> {
        let socket = udp_socket(SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0))?;
        let address = socket.local_addr()?;
        assert!(address.is_ipv6(), "this test requires local IPv6");
        let listener = tokio::net::UdpSocket::from_std(socket)?;
        for ip in [Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()] {
            let client = tokio::net::UdpSocket::bind(SocketAddr::new(ip, 0)).await?;
            client
                .send_to(b"dual-stack", SocketAddr::new(ip, address.port()))
                .await?;
            let mut bytes = [0u8; 32];
            let (len, _) =
                tokio::time::timeout(Duration::from_secs(2), listener.recv_from(&mut bytes))
                    .await??;
            assert_eq!(&bytes[..len], b"dual-stack");
        }
        drop(listener);
        let reopened = udp_socket(address)?;
        assert_eq!(reopened.local_addr()?, address);
        Ok(())
    }

    #[test]
    fn fallback_only_changes_an_unavailable_ipv6_wildcard() -> io::Result<()> {
        let wildcard = SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 34935);
        let result = bind_with_fallback(
            wildcard,
            |address| {
                if address.is_ipv6() {
                    Err(io::ErrorKind::Unsupported.into())
                } else {
                    Ok(address)
                }
            },
            || false,
        )?;
        assert_eq!(result, SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 34935));
        for address in [wildcard, SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 34935)] {
            let error = bind_with_fallback::<()>(
                address,
                |_| Err(io::ErrorKind::AddrInUse.into()),
                || false,
            )
            .err()
            .ok_or_else(|| io::Error::other("unexpected bind success"))?;
            assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        }
        let error = bind_with_fallback::<()>(
            SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 34935),
            |_| Err(io::ErrorKind::AddrNotAvailable.into()),
            || false,
        )
        .err()
        .ok_or_else(|| io::Error::other("unexpected bind success"))?;
        assert_eq!(error.kind(), io::ErrorKind::AddrNotAvailable);
        Ok(())
    }
}
