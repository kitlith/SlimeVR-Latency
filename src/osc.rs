use core::slice;
use std::io::ErrorKind;

use rosc::{decoder::decode_udp, OscPacket, OscMessage, address::{Matcher, OscAddress}};
use tokio::{net::UdpSocket, sync::mpsc::{self, error::TryRecvError}, time::Instant};

const MAX_PACKET_SIZE: usize = 1500;

pub async fn recv_osc(mut recv: mpsc::Receiver<(Instant, f32)>) -> std::io::Result<()> {
    // bind in place of VRChat
    let sock = UdpSocket::bind("0.0.0.0:9000").await?;
    let mut buf = vec![0u8; MAX_PACKET_SIZE];
    let head_pos_match = Matcher::new("/tracking/trackers/head/position").unwrap();
    let mut pending_test = None;
    loop {
        let msg_len = sock.recv(&mut buf).await?;
        let timestamp = Instant::now();
        let packet = match decode_udp(&buf[..msg_len]) {
            Ok((_, m)) => m,
            Err(e) => {
                println!("OSC Error: {}", e);
                continue;
            }
        };

        if pending_test.is_none() {
            match recv.try_recv() {
                Err(TryRecvError::Disconnected) => return Err(ErrorKind::NotConnected.into()),
                Err(TryRecvError::Empty) => {}
                Ok(test) => pending_test = Some(test),
            }
        }

        parse_packet(packet, &mut |msg| {
            match OscAddress::new(msg.addr) {
                Ok(a) if head_pos_match.match_address(&a) => {
                    if let Some((sent, pos_x)) = pending_test {
                        let x = msg.args[0].clone().float().unwrap();
                        if x == pos_x {
                            pending_test = None;
                            let duration = timestamp.duration_since(sent);
                            println!(
                                "driver -> server -> osc: {}ms ({}μs) since sent {} as x pos",
                                duration.as_millis(),
                                duration.as_micros(),
                                pos_x
                            );
                        }
                    }
                },
                _ => {},
            }
        });
    }
}

fn parse_packet(packet: OscPacket, handler: &mut impl FnMut(OscMessage)) {
    match packet {
        OscPacket::Message(m) => handler(m),
        OscPacket::Bundle(b) => for p in b.content.into_iter() {
            parse_packet(p, handler)
        },
    }
}