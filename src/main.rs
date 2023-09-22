use mio::net::UdpSocket;
use mio::{Events, Interest, Poll, Token};
use socket2::{Domain, Protocol, Socket, Type};
use std::io::{Error, ErrorKind};
use std::net::{Ipv4Addr, SocketAddr};
use std::str;

const SYSLOG_UDP_PORT: u16 = 514;
const SERVER: Token = Token(0);

fn main() -> Result<(), Error> {
    let mut events = Events::with_capacity(256);
    let mut poll = Poll::new()?;
    let mut buffer = [0u8; 1024];

    //Open UDP socket using socket2
    let udp4_server_s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    //Set listening address (0.0.0.0:514)
    let sa4 = SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), SYSLOG_UDP_PORT);

    //Enable address reuse and bind to address
    udp4_server_s.set_reuse_address(true)?;
    #[cfg(unix)]
    udp4_server_s.set_reuse_port(true)?; //Add for Unix CFG, does not work on Windows
    udp4_server_s.bind(&sa4.into())?;
    let mut udp4_server_mio = UdpSocket::from_std(udp4_server_s.into());

    //Register UDP socket with poll
    poll.registry()
        .register(&mut udp4_server_mio, SERVER, Interest::READABLE)?;
    let mut shutdown = false;
    while !shutdown {
        poll.poll(&mut events, None)?; //Poll for events
        for event in events.iter() {
            match event.token() {
                //Check for event token
                SERVER => match receive(&udp4_server_mio, &mut buffer) {
                    //Receive UDP packet
                    Ok(()) => continue,
                    Err(e) => {
                        eprintln!("Receive error: {}", e);
                        shutdown = true; //Shutdown if error
                    }
                },
                _ => shutdown = true, //Shutdown if other token
            }
        }
    }
    Ok(())
}

fn receive(sock: &UdpSocket, buf: &mut [u8]) -> Result<(), Error> {
    loop {
        //Get info about the sender
        let (_len, from) = match sock.recv_from(buf) {
            Ok((len, from)) => (len, from), //Return length and sender
            Err(e) => {
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted {
                    //If would block, return
                    return Ok(());
                } else {
                    return Err(e);
                }
            }
        };
        //Read the message
        let s = match str::from_utf8(buf) {
            Ok(v) => v,
            Err(e) => panic!("Not UTF-8: {}", e),
        };

        println!("[{}] {}", from, s);
        for elem in buf.iter_mut() {
            *elem = 0;
        } //reset array to clear buffer
    }
}
