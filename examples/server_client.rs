//! Note that the terms "client" and "server" here are purely what we logically associate with them.
//! Technically, they both work the same.
//! Note that in practice you don't want to implement a chat client using UDP.
use std::{io::stdin, net::SocketAddr};
use std::thread;
use coarsetime::Instant;

use laminar::{ErrorKind, Packet, Socket, SocketEvent};
use socket2::SockAddr;

const SERVER: &str = "127.0.0.1:12351";

fn server() -> Result<(), ErrorKind> {
    let mut socket = Socket::<coarsetime::Instant>::bind(SERVER, false)?;
    let (sender, receiver) = (socket.get_packet_sender(), socket.get_event_receiver());
    let _thread = thread::spawn(move || socket.start_polling());

    loop {
        if let Ok(event) = receiver.recv() {
            match event {
                SocketEvent::Packet(packet) => {
                    let msg = packet.payload();

                    if msg == b"Bye!" {
                        break;
                    }

                    let msg = String::from_utf8_lossy(msg);
                    
                    println!("Received {:?} from {:?}", msg, packet.addr());

                    sender
                        .send_and_poll(Packet::reliable_unordered(
                            packet.addr().clone(),
                            "Copy that!".as_bytes().to_vec(),
                            "user packet"
                        ))
                        .expect("This should send");
                }
                SocketEvent::Overload(address) => {
                    println!("Client overloaded: {:?}", address);
                }
                SocketEvent::Timeout(address) => {
                    println!("Client timed out: {:?}", address);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn client() -> Result<(), ErrorKind> {
    let addr = "127.0.0.1:12352";
    let mut socket = Socket::bind(addr, true)?;
    println!("Connected on {}", addr);

    let server: SocketAddr = SERVER.parse().unwrap();
    let server: SockAddr = server.into();

    println!("Type a message and press Enter to send. Send `Bye!` to quit.");

    let stdin = stdin();
    let mut s_buffer = String::new();

    loop {
        s_buffer.clear();
        stdin.read_line(&mut s_buffer)?;
        let line = s_buffer.replace(|x| x == '\n' || x == '\r', "");

        socket.send(Packet::reliable_unordered(
            server.clone(),
            line.clone().into_bytes(),
            "user packet"
        ))?;

        socket.manual_poll(Instant::now()).unwrap();

        if line == "Bye!" {
            break;
        }

        match socket.recv() {
            Some(SocketEvent::Packet(packet)) => {
                if packet.addr().clone() == server {
                    println!("Server sent: {}", String::from_utf8_lossy(packet.payload()));
                } else {
                    println!("Unknown sender.");
                }
            }
            Some(SocketEvent::Timeout(_)) => {}
            _ => println!("Silence.."),
        }
    }

    Ok(())
}

fn main() -> Result<(), ErrorKind> {
    let stdin = stdin();

    println!("Please type in `server` or `client`.");

    let mut s = String::new();
    stdin.read_line(&mut s)?;

    if s.starts_with('s') {
        println!("Starting server..");
        server()
    } else {
        println!("Starting client..");
        client()
    }
}
