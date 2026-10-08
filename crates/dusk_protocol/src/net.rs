//! Blocking TCP transport: one reader and one writer thread per connection,
//! exposed as channels so game loops (Bevy systems) only ever poll.

use crate::{ClientMsg, MAX_FRAME, ServerMsg, decode, encode};
use crossbeam_channel::{Receiver, Sender, unbounded};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::thread;

/// A live connection. `incoming` disconnects when the peer goes away or sends garbage.
pub struct Connection<In, Out> {
    pub incoming: Receiver<In>,
    pub outgoing: Sender<Out>,
    pub peer: String,
    stream: TcpStream,
}

impl<In, Out> Connection<In, Out> {
    pub fn send(&self, msg: Out) {
        // A send error only means the writer thread is gone; `incoming` reports that.
        let _ = self.outgoing.send(msg);
    }

    pub fn close(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

impl<In, Out> Drop for Connection<In, Out> {
    fn drop(&mut self) {
        self.close();
    }
}

pub type ClientConnection = Connection<ServerMsg, ClientMsg>;
pub type ServerConnection = Connection<ClientMsg, ServerMsg>;

fn read_frame(stream: &mut TcpStream, buf: &mut Vec<u8>) -> io::Result<()> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("frame too large: {len}")));
    }
    buf.resize(len, 0);
    stream.read_exact(buf)
}

pub fn wrap<In, Out>(stream: TcpStream) -> io::Result<Connection<In, Out>>
where
    In: DeserializeOwned + Send + 'static,
    Out: Serialize + Send + 'static,
{
    stream.set_nodelay(true)?;
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    let (in_tx, in_rx) = unbounded::<In>();
    let (out_tx, out_rx) = unbounded::<Out>();

    let mut reader = stream.try_clone()?;
    thread::Builder::new().name(format!("net-read {peer}")).spawn(move || {
        let mut buf = Vec::new();
        while read_frame(&mut reader, &mut buf).is_ok() {
            let Ok(msg) = decode::<In>(&buf) else { break };
            if in_tx.send(msg).is_err() {
                break;
            }
        }
        let _ = reader.shutdown(Shutdown::Both);
    })?;

    let mut writer = stream.try_clone()?;
    thread::Builder::new().name(format!("net-write {peer}")).spawn(move || {
        for msg in out_rx {
            if writer.write_all(&encode(&msg)).is_err() {
                break;
            }
        }
        let _ = writer.shutdown(Shutdown::Both);
    })?;

    Ok(Connection { incoming: in_rx, outgoing: out_tx, peer, stream })
}

pub fn connect(addr: impl ToSocketAddrs) -> io::Result<ClientConnection> {
    wrap(TcpStream::connect(addr)?)
}

/// Accepts connections on a background thread; new connections arrive on the channel.
/// Returns the bound address too (useful with port 0).
pub fn listen(addr: impl ToSocketAddrs) -> io::Result<(Receiver<ServerConnection>, SocketAddr)> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    let (tx, rx) = unbounded();
    thread::Builder::new().name("net-accept".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            match wrap(stream) {
                Ok(conn) => {
                    if tx.send(conn).is_err() {
                        break;
                    }
                }
                Err(e) => eprintln!("accept failed: {e}"),
            }
        }
    })?;
    Ok((rx, local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PROTOCOL_VERSION, Pos};
    use std::time::Duration;

    #[test]
    fn client_server_exchange() {
        let (accepted, addr) = listen("127.0.0.1:0").unwrap();
        let client = connect(addr).unwrap();
        let server = accepted.recv_timeout(Duration::from_secs(5)).unwrap();

        client.send(ClientMsg::Hello { protocol: PROTOCOL_VERSION, name: "a".into(), class: 1 });
        let got = server.incoming.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, ClientMsg::Hello { protocol: PROTOCOL_VERSION, name: "a".into(), class: 1 });

        server.send(ServerMsg::Correct { pos: Pos { x: 1.0, y: 2.0 } });
        let got = client.incoming.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, ServerMsg::Correct { pos: Pos { x: 1.0, y: 2.0 } });

        drop(server);
        assert!(client.incoming.recv_timeout(Duration::from_secs(5)).is_err());
    }
}
