include!(concat!(env!("OUT_DIR"), "/protos/mod.rs"));

use std::collections::HashMap;

use protobuf::{Message, SpecialFields};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, ErrorKind, AsyncWrite},
    net::windows::named_pipe::{ClientOptions},
    sync::mpsc::{self, error::TryRecvError},
    time::{sleep, Duration, Instant, MissedTickBehavior},
};

pub async fn recreate_client_loop() -> std::io::Result<()> {
    loop {
        let mut buf = Vec::new();
        let mut client = match ClientOptions::new().open(PIPE_NAME) {
            Ok(c) => c,
            Err(_) => {
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        {
            // Send HMD add messages
            let mut msg = ProtobufMessages::ProtobufMessage::new();
            msg.set_tracker_added(ProtobufMessages::TrackerAdded {
                tracker_id: 0,
                tracker_serial: "SlimeVR Latency Test".to_string(),
                tracker_name: "Latency Tester".to_string(),
                tracker_role: 19, // TrackerRole::HMD
                special_fields: SpecialFields::new(),
            });
            buf.extend_from_slice(&((msg.compute_size() + 4) as u32).to_le_bytes());
            msg.write_to_vec(&mut buf)?;

            msg.set_tracker_status(ProtobufMessages::TrackerStatus {
                tracker_id: 0,
                status: ProtobufMessages::tracker_status::Status::OK.into(),
                extra: HashMap::new(),
                confidence: Some(ProtobufMessages::tracker_status::Confidence::HIGH.into()),
                special_fields: SpecialFields::new(),
            });
            buf.extend_from_slice(&((msg.compute_size() + 4) as u32).to_le_bytes());
            msg.write_to_vec(&mut buf)?;
            client.write_all(&buf).await?;
        }

        let (reader, writer) = tokio::io::split(client);
        let (bridge_s, bridge_r) = mpsc::channel(1);
        let (osc_send, osc_recv) = mpsc::channel(1);

        let mut task_set = tokio::task::JoinSet::new();
        task_set.spawn(bridge_recv(reader, bridge_r));
        task_set.spawn(bridge_send(writer, bridge_s, osc_send));
        task_set.spawn(crate::osc::recv_osc(osc_recv));
        // when a task exits, that means an IO error has occurred.
        println!("task died with error: {}", task_set.join_next().await.unwrap().unwrap().unwrap_err());
        // so we immediately abort and create a new connection
        drop(task_set);
        // after waiting a bit to allow the other end to detect the closure
        sleep(Duration::from_secs(1)).await;
    }
}

async fn bridge_recv(
    reader: impl AsyncRead + Unpin,
    mut recv: mpsc::Receiver<(Instant, f32)>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut pending_test: Option<(Instant, f32)> = None;
    let mut buf = Vec::new();
    loop {
        let msg_len = reader.read_u32_le().await? as usize - 4;
        let timestamp = Instant::now();

        // possible reallocation and buffer initialization
        buf.resize(msg_len, 0u8);
        let buf = &mut buf[..msg_len];

        reader.read_exact(buf).await?;

        let msg = ProtobufMessages::ProtobufMessage::parse_from_bytes(buf)?;

        match msg.message {
            None => {}
            Some(ProtobufMessages::protobuf_message::Message::Position(position)) => {
                if pending_test.is_none() {
                    match recv.try_recv() {
                        Err(TryRecvError::Disconnected) => return Err(ErrorKind::NotConnected.into()),
                        Err(TryRecvError::Empty) => {}
                        Ok(test) => pending_test = Some(test),
                    }
                }
                if let Some((sent, pos_x)) = pending_test {
                    if Some(pos_x) == position.x {
                        pending_test = None;
                        let duration = timestamp.duration_since(sent);
                        println!(
                            "driver -> server -> driver: {}ms ({}μs) since sent {} as x pos",
                            duration.as_millis(),
                            duration.as_micros(),
                            pos_x
                        );
                    }
                }
            }
            Some(_) => {}
        }
    }
}

async fn bridge_send(mut writer: impl AsyncWrite + Unpin, bridge_send: mpsc::Sender<(Instant, f32)>, osc_send: mpsc::Sender<(Instant, f32)>) -> std::io::Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut buf = Vec::new();

    let mut msg = ProtobufMessages::ProtobufMessage::new();
    msg.set_position(ProtobufMessages::Position {
        tracker_id: 0,
        x: Some(0f32),
        y: Some(0f32),
        z: Some(0f32),
        qx: 0f32,
        qy: 0f32,
        qz: 0f32,
        qw: 1f32,
        data_source: Some(ProtobufMessages::position::DataSource::FULL.into()),
        special_fields: SpecialFields::new(),
    });

    loop {
        interval.tick().await;

        *msg.mut_position().x.as_mut().unwrap() += 1f32;
        
        buf.clear();
        buf.extend_from_slice(&((msg.compute_size() + 4) as u32).to_le_bytes());
        msg.write_to_vec(&mut buf)?;

        // TODO: where's the best spot to measure the time from?
        // let timestamp = Instant::now();
        writer.write_all(&buf).await?;
        let timestamp = Instant::now();
        let test = (timestamp, msg.position().x.unwrap());

        if let Err(_) = tokio::try_join!(bridge_send.send(test), osc_send.send(test)) {
            return Err(ErrorKind::NotConnected.into());
        }
    }
}

const PIPE_NAME: &'static str = "\\\\.\\pipe\\SlimeVRDriver";

