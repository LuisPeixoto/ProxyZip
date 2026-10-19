//! A SOCKS5 listener (RFC 1928, no authentication, CONNECT only). After the handshake the connection goes through
//! the same pipeline as an HTTP `CONNECT`, so TLS interception, rules and recording all apply.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::server::Ctx;
use crate::tunnel;

const VERSION: u8 = 5;
const NO_AUTH: u8 = 0x00;
const NO_ACCEPTABLE_METHOD: u8 = 0xff;
const CMD_CONNECT: u8 = 0x01;
const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;
const REPLY_SUCCEEDED: u8 = 0x00;
const REPLY_COMMAND_NOT_SUPPORTED: u8 = 0x07;
const REPLY_ADDRESS_TYPE_NOT_SUPPORTED: u8 = 0x08;

pub(crate) async fn handle(mut stream: TcpStream, peer: SocketAddr, ctx: Arc<Ctx>) {
    let _ = stream.set_nodelay(true);
    match negotiate(&mut stream).await {
        Ok(Some((host, port))) => tunnel::handle(stream, host, port, ctx, peer).await,
        Ok(None) => {}
        Err(e) => tracing::debug!(error = %e, %peer, "SOCKS5 handshake failed"),
    }
}

/// Run the handshake. `Ok(None)` means the client was refused politely.
async fn negotiate(stream: &mut TcpStream) -> std::io::Result<Option<(String, u16)>> {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await?;
    if head[0] != VERSION {
        return Ok(None);
    }
    let mut methods = vec![0u8; head[1] as usize];
    stream.read_exact(&mut methods).await?;
    if !methods.contains(&NO_AUTH) {
        stream.write_all(&[VERSION, NO_ACCEPTABLE_METHOD]).await?;
        return Ok(None);
    }
    stream.write_all(&[VERSION, NO_AUTH]).await?;

    let mut request = [0u8; 4];
    stream.read_exact(&mut request).await?;
    let [version, command, _reserved, atyp] = request;
    if version != VERSION {
        return Ok(None);
    }
    let host = match atyp {
        ATYP_IPV4 => {
            let mut octets = [0u8; 4];
            stream.read_exact(&mut octets).await?;
            Ipv4Addr::from(octets).to_string()
        }
        ATYP_IPV6 => {
            let mut octets = [0u8; 16];
            stream.read_exact(&mut octets).await?;
            Ipv6Addr::from(octets).to_string()
        }
        ATYP_DOMAIN => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut name = vec![0u8; len[0] as usize];
            stream.read_exact(&mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        _ => {
            reply(stream, REPLY_ADDRESS_TYPE_NOT_SUPPORTED).await?;
            return Ok(None);
        }
    };
    let mut port = [0u8; 2];
    stream.read_exact(&mut port).await?;
    if command != CMD_CONNECT {
        reply(stream, REPLY_COMMAND_NOT_SUPPORTED).await?;
        return Ok(None);
    }
    reply(stream, REPLY_SUCCEEDED).await?;
    Ok(Some((host, u16::from_be_bytes(port))))
}

/// The bound address in replies is not meaningful here, so it is always `0.0.0.0:0`.
async fn reply(stream: &mut TcpStream, code: u8) -> std::io::Result<()> {
    stream.write_all(&[VERSION, code, 0, ATYP_IPV4, 0, 0, 0, 0, 0, 0]).await
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    async fn handshake(client_bytes: Vec<u8>) -> (Option<(String, u16)>, Vec<u8>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            negotiate(&mut s).await.unwrap()
        });
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(&client_bytes).await.unwrap();
        let result = server.await.unwrap();
        let mut received = Vec::new();
        client.read_to_end(&mut received).await.unwrap();
        (result, received)
    }

    #[tokio::test]
    async fn connect_by_domain() {
        let mut bytes = vec![5, 1, 0, 5, 1, 0, 3, 11];
        bytes.extend_from_slice(b"example.com");
        bytes.extend_from_slice(&443u16.to_be_bytes());
        let (target, received) = handshake(bytes).await;
        assert_eq!(target, Some(("example.com".to_string(), 443)));
        assert_eq!(received, [5, 0, 5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    }

    #[tokio::test]
    async fn connect_by_ipv4_and_ipv6() {
        let mut v4 = vec![5, 1, 0, 5, 1, 0, 1, 127, 0, 0, 1];
        v4.extend_from_slice(&8080u16.to_be_bytes());
        assert_eq!(handshake(v4).await.0, Some(("127.0.0.1".to_string(), 8080)));
        let mut v6 = vec![5, 1, 0, 5, 1, 0, 4];
        v6.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        v6.extend_from_slice(&80u16.to_be_bytes());
        assert_eq!(handshake(v6).await.0, Some(("::1".to_string(), 80)));
    }

    #[tokio::test]
    async fn refuses_authentication_only_clients_and_other_commands() {
        let (target, received) = handshake(vec![5, 1, 2]).await;
        assert_eq!(target, None);
        assert_eq!(received, [5, 0xff]);
        let mut bind = vec![5, 1, 0, 5, 2, 0, 1, 127, 0, 0, 1];
        bind.extend_from_slice(&80u16.to_be_bytes());
        let (target, received) = handshake(bind).await;
        assert_eq!(target, None);
        assert_eq!(&received[2..4], [5, REPLY_COMMAND_NOT_SUPPORTED]);
    }
}
