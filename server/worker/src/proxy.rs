//! A sandbox-local loopback bridge to the single mounted host broker socket.

use std::{
    io,
    net::{Shutdown, TcpListener},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

/// There is no outbound network: every connection reaches only this Unix socket.
pub fn start(socket: PathBuf) -> io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            if active.fetch_add(1, Ordering::AcqRel) >= 32 {
                active.fetch_sub(1, Ordering::AcqRel);
                continue;
            }
            let (socket, active) = (socket.clone(), Arc::clone(&active));
            std::thread::spawn(move || {
                let _ = client.set_read_timeout(Some(Duration::from_mins(5)));
                let _ = client.set_write_timeout(Some(Duration::from_mins(5)));
                if let Ok(broker) = UnixStream::connect(socket) {
                    let _ = broker.set_read_timeout(Some(Duration::from_mins(5)));
                    let _ = broker.set_write_timeout(Some(Duration::from_mins(5)));
                    if let (Ok(mut request), Ok(mut upstream)) =
                        (client.try_clone(), broker.try_clone())
                    {
                        let writer = std::thread::spawn(move || {
                            let _ = io::copy(&mut request, &mut upstream);
                            let _ = upstream.shutdown(Shutdown::Write);
                        });
                        let (mut response, mut downstream) = (broker, client);
                        let _ = io::copy(&mut response, &mut downstream);
                        let _ = downstream.shutdown(Shutdown::Both);
                        let _ = writer.join();
                    }
                }
                active.fetch_sub(1, Ordering::AcqRel);
            });
        }
    });
    Ok(port)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpStream,
        os::unix::net::UnixListener,
    };

    #[test]
    fn forwards_only_to_the_named_unix_socket() {
        let dir = std::env::temp_dir().join(format!("loamz-proxy-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("socket");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            let mut input = [0_u8; 4];
            connection.read_exact(&mut input).unwrap();
            assert_eq!(&input, b"ping");
            connection.write_all(b"pong").unwrap();
        });
        let port = start(socket).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(b"ping").unwrap();
        let mut output = [0_u8; 4];
        client.read_exact(&mut output).unwrap();
        assert_eq!(&output, b"pong");
        drop(client);
        server.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
