//! Port forwarding, written as OpenSSH's options are:
//!
//! - `-L [bind:]port:host:hostport`: connections to `port` here are carried to
//!   `host:hostport`, which the server connects to.
//! - `-R [bind:]port:host:hostport`: the server listens on `port`, and its
//!   connections come back here to be connected to `host:hostport`.
//! - `-D [bind:]port`: a SOCKS 4 and 5 proxy here; each connection goes wherever
//!   it asks, from the server.
//!
//! As OpenSSH without GatewayPorts, ports listen on loopback unless a bind
//! address says otherwise; `*` means every interface. Port 0 takes any free one.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{ChannelOpenHandle, Handle, Msg};
use russh::{Channel, ChannelOpenFailure};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::AbortHandle;

use crate::Error;
use crate::hostkey::Checker;

/// Where a forward listens. `host` None is loopback, `*` every interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listen {
    pub host: Option<String>,
    pub port: u16,
}

/// Where forwarded connections go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dest {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Forward {
    Local { listen: Listen, to: Dest },
    Remote { listen: Listen, to: Dest },
    Dynamic { listen: Listen },
}

/// Splits on colons outside brackets, dropping the brackets: `[::1]:80` is
/// `::1` and `80`.
fn split_colons(s: &str) -> Option<Vec<String>> {
    let (mut parts, mut cur, mut bracket) = (vec![], String::new(), false);
    for c in s.chars() {
        match c {
            '[' if !bracket && cur.is_empty() => bracket = true,
            ']' if bracket => bracket = false,
            ':' if !bracket => parts.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    if bracket {
        return None;
    }
    parts.push(cur);
    Some(parts)
}

impl FromStr for Forward {
    type Err = Error;

    /// Reads `-L 8080:localhost:80`, `-R 9000:localhost:3000`, `-D 1080` and the
    /// like; the dash and the space after the letter may be left out.
    fn from_str(spec: &str) -> Result<Forward, Error> {
        let spec = spec.trim();
        let bad = |why: &str| Error::BadForward {
            spec: spec.to_string(),
            why: why.to_string(),
        };
        let s = spec.strip_prefix('-').unwrap_or(spec);
        let mut chars = s.chars();
        let kind = chars.next().ok_or_else(|| bad("it is empty"))?;
        let parts = split_colons(chars.as_str().trim()).ok_or_else(|| bad("a [ is not closed"))?;
        let port = |p: &str| {
            p.parse::<u16>()
                .map_err(|_| bad(&format!("{p:?} is not a port")))
        };
        let listen = |bind: Option<&String>, p: &str| -> Result<Listen, Error> {
            let host = match bind.map(String::as_str) {
                None | Some("localhost") => None,
                Some("" | "*") => Some("*".to_string()),
                Some(h) => Some(h.to_string()),
            };
            Ok(Listen {
                host,
                port: port(p)?,
            })
        };
        let dest = |h: &str, p: &str| -> Result<Dest, Error> {
            match port(p)? {
                _ if h.is_empty() => Err(bad("the host to connect to is missing")),
                0 => Err(bad("port 0 cannot be connected to")),
                port => Ok(Dest {
                    host: h.to_string(),
                    port,
                }),
            }
        };
        let p = &parts;
        match (kind, p.len()) {
            ('L' | 'R', 3 | 4) => {
                let bind = (p.len() == 4).then(|| &p[0]);
                let i = p.len() - 3;
                let (listen, to) = (listen(bind, &p[i])?, dest(&p[i + 1], &p[i + 2])?);
                Ok(if kind == 'L' {
                    Forward::Local { listen, to }
                } else {
                    Forward::Remote { listen, to }
                })
            }
            ('R', 1 | 2) => Err(bad(
                "a SOCKS proxy on the server (-R without a destination) is not supported",
            )),
            ('L' | 'R', _) => Err(bad("expected [bind:]port:host:hostport")),
            ('D', 1) => Ok(Forward::Dynamic {
                listen: listen(None, &p[0])?,
            }),
            ('D', 2) => Ok(Forward::Dynamic {
                listen: listen(Some(&p[0]), &p[1])?,
            }),
            ('D', _) => Err(bad("expected [bind:]port")),
            _ => Err(bad("expected -L, -R or -D")),
        }
    }
}

/// A host as written in a spec: IPv6 addresses in brackets.
fn host(h: &str) -> String {
    if h.contains(':') {
        format!("[{h}]")
    } else {
        h.to_string()
    }
}

impl fmt::Display for Listen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = self.host.as_deref().unwrap_or("localhost");
        write!(f, "{}:{}", host(h), self.port)
    }
}

impl fmt::Display for Dest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", host(&self.host), self.port)
    }
}

/// The spec again, as `-L localhost:8080:localhost:80`.
impl fmt::Display for Forward {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Forward::Local { listen, to } => write!(f, "-L {listen}:{to}"),
            Forward::Remote { listen, to } => write!(f, "-R {listen}:{to}"),
            Forward::Dynamic { listen } => write!(f, "-D {listen}"),
        }
    }
}

/// A forward that runs, on the port it got (the one asked for unless that was 0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Active {
    pub forward: Forward,
    pub port: u16,
}

impl Active {
    /// The forward on the port it got: what to ask for to get the same again.
    pub fn running(&self) -> Forward {
        let mut f = self.forward.clone();
        match &mut f {
            Forward::Local { listen, .. }
            | Forward::Remote { listen, .. }
            | Forward::Dynamic { listen } => listen.port = self.port,
        }
        f
    }
}

impl fmt::Display for Active {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on = |l: &Listen| Listen {
            host: l.host.clone(),
            port: self.port,
        };
        match &self.forward {
            Forward::Local { listen, to } => write!(f, "forwarding {} → {to}", on(listen)),
            Forward::Remote { listen, to } => {
                write!(f, "forwarding the server's {} → {to}", on(listen))
            }
            Forward::Dynamic { listen } => write!(f, "SOCKS proxy on {}", on(listen)),
        }
    }
}

/// Where the server's forwarded connections go, by the address and port the
/// server listens on. Shared with the connection's handler.
#[derive(Clone, Default)]
pub(crate) struct Routes(Arc<Mutex<Vec<(String, u32, Dest)>>>);

impl Routes {
    fn add(&self, address: &str, port: u32, to: Dest) {
        if let Ok(mut r) = self.0.lock() {
            r.push((address.to_string(), port, to));
        }
    }

    fn find(&self, address: &str, port: u32) -> Option<Dest> {
        let routes = self.0.lock().ok()?;
        // Servers name the address as it was asked for; the port alone decides
        // if one names it differently.
        routes
            .iter()
            .find(|(a, p, _)| *p == port && a == address)
            .or_else(|| routes.iter().find(|(_, p, _)| *p == port))
            .map(|(_, _, to)| to.clone())
    }

    /// A connection the server forwards to us: connected to where its route
    /// says, or refused.
    pub(crate) fn open(
        &self,
        channel: Channel<Msg>,
        address: &str,
        port: u32,
        reply: ChannelOpenHandle,
    ) {
        let Some(to) = self.find(address, port) else {
            // Nothing asked for this one: dropping the reply refuses it.
            return;
        };
        tokio::spawn(async move {
            match TcpStream::connect((to.host.as_str(), to.port)).await {
                Ok(mut tcp) => {
                    reply.accept().await;
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
                }
                Err(_) => reply.reject(ChannelOpenFailure::ConnectFailed).await,
            }
        });
    }
}

/// The running forwards of one connection. Dropping it closes their ports.
pub(crate) struct Forwarder {
    handle: Arc<Handle<Checker>>,
    routes: Routes,
    listeners: Vec<AbortHandle>,
}

impl Forwarder {
    pub(crate) fn new(handle: Arc<Handle<Checker>>, routes: Routes) -> Self {
        Forwarder {
            handle,
            routes,
            listeners: vec![],
        }
    }

    pub(crate) async fn start(&mut self, forward: &Forward) -> Result<Active, Error> {
        let fail = |why: String| Error::Forward {
            spec: forward.to_string(),
            why,
        };
        match forward {
            Forward::Local { listen, to } => self
                .listen(listen, Some(to.clone()))
                .await
                .map_err(|e| fail(e.to_string()))
                .map(|port| Active {
                    forward: forward.clone(),
                    port,
                }),
            Forward::Dynamic { listen } => self
                .listen(listen, None)
                .await
                .map_err(|e| fail(e.to_string()))
                .map(|port| Active {
                    forward: forward.clone(),
                    port,
                }),
            Forward::Remote { listen, to } => {
                // What OpenSSH asks for: loopback unless told, "" for every interface.
                let address = match listen.host.as_deref() {
                    None => "localhost",
                    Some("*") => "",
                    Some(h) => h,
                };
                let got = self
                    .handle
                    .tcpip_forward(address, listen.port as u32)
                    .await
                    .map_err(|e| match e {
                        russh::Error::RequestDenied => fail(
                            "the server refused (the port may be taken, or forwarding turned off)"
                                .into(),
                        ),
                        e => fail(e.to_string()),
                    })?;
                // The reply names the port only when any was asked for.
                let port = if got == 0 { listen.port as u32 } else { got };
                self.routes.add(address, port, to.clone());
                Ok(Active {
                    forward: forward.clone(),
                    port: port as u16,
                })
            }
        }
    }

    /// Listens for a local or SOCKS forward (`to` None) and returns the port.
    async fn listen(&mut self, listen: &Listen, to: Option<Dest>) -> std::io::Result<u16> {
        // Loopback and every interface mean both IPv4 and IPv6, where there is IPv6.
        let hosts: Vec<&str> = match listen.host.as_deref() {
            None => vec!["127.0.0.1", "::1"],
            Some("*") => vec!["0.0.0.0", "::"],
            Some(h) => vec![h],
        };
        let first = TcpListener::bind((hosts[0], listen.port)).await?;
        let port = first.local_addr()?.port();
        let mut all = vec![first];
        for h in &hosts[1..] {
            if let Ok(l) = TcpListener::bind((*h, port)).await {
                all.push(l);
            }
        }
        for listener in all {
            let (handle, to) = (self.handle.clone(), to.clone());
            let task = tokio::spawn(async move {
                loop {
                    let (sock, peer) = match listener.accept().await {
                        Ok(c) => c,
                        // Out of file descriptors and the like: wait, then go on.
                        Err(_) => {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                    };
                    let (handle, to) = (handle.clone(), to.clone());
                    tokio::spawn(async move {
                        let _ = match to {
                            Some(to) => carry(&handle, sock, peer, &to).await,
                            None => socks(&handle, sock, peer).await,
                        };
                    });
                }
            });
            self.listeners.push(task.abort_handle());
        }
        Ok(port)
    }
}

impl Drop for Forwarder {
    fn drop(&mut self) {
        for l in &self.listeners {
            l.abort();
        }
    }
}

async fn open(
    handle: &Handle<Checker>,
    to: &Dest,
    peer: SocketAddr,
) -> Result<Channel<Msg>, russh::Error> {
    handle
        .channel_open_direct_tcpip(
            to.host.clone(),
            to.port as u32,
            peer.ip().to_string(),
            peer.port() as u32,
        )
        .await
}

async fn pipe(mut sock: TcpStream, channel: Channel<Msg>) -> std::io::Result<()> {
    let mut stream = channel.into_stream();
    tokio::io::copy_bidirectional(&mut sock, &mut stream).await?;
    Ok(())
}

/// One local connection, carried to `to` through the server.
async fn carry(
    handle: &Handle<Checker>,
    sock: TcpStream,
    peer: SocketAddr,
    to: &Dest,
) -> std::io::Result<()> {
    match open(handle, to, peer).await {
        Ok(channel) => pipe(sock, channel).await,
        // Closing the socket is all a plain TCP client can be told.
        Err(_) => Ok(()),
    }
}

/// Reads bytes up to a NUL, as SOCKS 4 strings are sent.
async fn read_cstr(sock: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut out = vec![];
    loop {
        match sock.read_u8().await? {
            0 => return Ok(out),
            _ if out.len() >= 255 => {
                return Err(std::io::Error::other("SOCKS string too long"));
            }
            b => out.push(b),
        }
    }
}

/// SOCKS 5 reply codes for a channel the server would not open.
fn socks5_code(e: &russh::Error) -> u8 {
    match e {
        russh::Error::ChannelOpenFailure(ChannelOpenFailure::AdministrativelyProhibited) => 2,
        russh::Error::ChannelOpenFailure(ChannelOpenFailure::ConnectFailed) => 5,
        _ => 1,
    }
}

/// One SOCKS connection: CONNECT only, no authentication, as OpenSSH's -D.
async fn socks(
    handle: &Handle<Checker>,
    mut sock: TcpStream,
    peer: SocketAddr,
) -> std::io::Result<()> {
    match sock.read_u8().await? {
        5 => {
            let mut methods = vec![0u8; sock.read_u8().await? as usize];
            sock.read_exact(&mut methods).await?;
            if !methods.contains(&0) {
                return sock.write_all(&[5, 0xff]).await;
            }
            sock.write_all(&[5, 0]).await?;
            let mut head = [0u8; 4]; // version, command, reserved, address type
            sock.read_exact(&mut head).await?;
            let host = match head[3] {
                1 => {
                    let mut a = [0u8; 4];
                    sock.read_exact(&mut a).await?;
                    Ipv4Addr::from(a).to_string()
                }
                3 => {
                    let mut name = vec![0u8; sock.read_u8().await? as usize];
                    sock.read_exact(&mut name).await?;
                    String::from_utf8_lossy(&name).into_owned()
                }
                4 => {
                    let mut a = [0u8; 16];
                    sock.read_exact(&mut a).await?;
                    Ipv6Addr::from(a).to_string()
                }
                _ => return reply5(&mut sock, 8).await,
            };
            let port = sock.read_u16().await?;
            if head[0] != 5 || head[1] != 1 {
                return reply5(&mut sock, 7).await;
            }
            match open(handle, &Dest { host, port }, peer).await {
                Ok(channel) => {
                    reply5(&mut sock, 0).await?;
                    pipe(sock, channel).await
                }
                Err(e) => reply5(&mut sock, socks5_code(&e)).await,
            }
        }
        4 => {
            let command = sock.read_u8().await?;
            let port = sock.read_u16().await?;
            let mut ip = [0u8; 4];
            sock.read_exact(&mut ip).await?;
            let _user = read_cstr(&mut sock).await?;
            // SOCKS 4a: 0.0.0.x (x not 0) means a host name follows.
            let host = if ip[..3] == [0, 0, 0] && ip[3] != 0 {
                String::from_utf8_lossy(&read_cstr(&mut sock).await?).into_owned()
            } else {
                Ipv4Addr::from(ip).to_string()
            };
            let reply = |ok: bool| [0, if ok { 0x5a } else { 0x5b }, 0, 0, 0, 0, 0, 0];
            if command != 1 {
                return sock.write_all(&reply(false)).await;
            }
            match open(handle, &Dest { host, port }, peer).await {
                Ok(channel) => {
                    sock.write_all(&reply(true)).await?;
                    pipe(sock, channel).await
                }
                Err(_) => sock.write_all(&reply(false)).await,
            }
        }
        _ => Ok(()),
    }
}

async fn reply5(sock: &mut TcpStream, code: u8) -> std::io::Result<()> {
    sock.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Forward, Error> {
        s.parse()
    }

    fn listen(host: Option<&str>, port: u16) -> Listen {
        Listen {
            host: host.map(String::from),
            port,
        }
    }

    fn dest(host: &str, port: u16) -> Dest {
        Dest {
            host: host.into(),
            port,
        }
    }

    #[test]
    fn reads_openssh_specs() {
        assert_eq!(
            parse("-L 8080:localhost:80").ok(),
            Some(Forward::Local {
                listen: listen(None, 8080),
                to: dest("localhost", 80)
            })
        );
        assert_eq!(
            parse("L0.0.0.0:8080:db.internal:5432").ok(),
            Some(Forward::Local {
                listen: listen(Some("0.0.0.0"), 8080),
                to: dest("db.internal", 5432)
            })
        );
        assert_eq!(
            parse("-R *:9000:[::1]:3000").ok(),
            Some(Forward::Remote {
                listen: listen(Some("*"), 9000),
                to: dest("::1", 3000)
            })
        );
        assert_eq!(
            parse("-D 1080").ok(),
            Some(Forward::Dynamic {
                listen: listen(None, 1080)
            })
        );
        assert_eq!(
            parse("-D localhost:1080").ok(),
            Some(Forward::Dynamic {
                listen: listen(None, 1080)
            })
        );
        assert_eq!(
            parse("-D :1080").ok(),
            Some(Forward::Dynamic {
                listen: listen(Some("*"), 1080)
            })
        );
        // Written back the long way, which reads the same again.
        let f = parse("-R *:9000:[::1]:3000").expect("parse");
        assert_eq!(f.to_string(), "-R *:9000:[::1]:3000");
        assert_eq!(parse(&f.to_string()).ok(), Some(f));
    }

    #[test]
    fn says_what_is_wrong() {
        for (spec, why) in [
            ("", "empty"),
            ("-L 8080", "[bind:]port:host:hostport"),
            ("-L 80800:localhost:80", "\"80800\" is not a port"),
            ("-L 8080::80", "host to connect to is missing"),
            ("-L 8080:h:0", "port 0"),
            ("-R 1080", "not supported"),
            ("-D a:b:c", "[bind:]port"),
            ("-X 1", "-L, -R or -D"),
            ("-L [::1:80:h:1", "not closed"),
        ] {
            let e = parse(spec).expect_err(spec).to_string();
            assert!(e.contains(why), "{spec:?}: {e}");
        }
    }

    #[test]
    fn describes_what_runs() {
        let active = |f: &str, port| Active {
            forward: parse(f).expect("parse"),
            port,
        };
        assert_eq!(
            active("-L 0:localhost:80", 41234).to_string(),
            "forwarding localhost:41234 → localhost:80"
        );
        assert_eq!(
            active("-R 9000:localhost:3000", 9000).to_string(),
            "forwarding the server's localhost:9000 → localhost:3000"
        );
        assert_eq!(
            active("-D *:1080", 1080).to_string(),
            "SOCKS proxy on *:1080"
        );
        assert_eq!(
            active("-D 0", 41000).running().to_string(),
            "-D localhost:41000"
        );
    }
}
