use mio::net::UdpSocket;
use mio::{Events, Poll, Token, Interest};
use std::io::ErrorKind;

const SERVER: Token = Token(0);

fn main() -> Result<(), std::io::Error> {
    let mut events = Events::with_capacity(256);
    let mut poll = Poll::new()?;
    let mut buffer = [0u8; 4096];

    let mut socket = UdpSocket::bind("127.0.0.1:514".parse().expect("parse failed"))?;

    poll.registry().register(&mut socket, Token(0), Interest::READABLE)?;

    let mut shutdown = false;
    while !shutdown {
        poll.poll(&mut events, None)?;
        for event in events.iter() {
            match event.token() {
                SERVER => {
                    match socket.recv(&mut buffer) {
                        Ok(len) => println!("recv {} bytes", len),
                        Err(e) => {
                            if e.kind() == ErrorKind::WouldBlock
                                || e.kind() == ErrorKind::Interrupted
                            {
                                continue;
                            } else {
                                println!("recv error: {:?}", e);
                                shutdown = true;
                            }
                        }
                    }
                }
                _ => (),
            }
        }
    }
    Ok(())
}