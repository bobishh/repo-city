//! Native room media. Room-authorized SDP/ICE travels through the room adapter.
//! Audio capture opens only after Enable; push-to-talk gates every outgoing frame.
use super::super::agent_engine::{audio, opus_codec};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use webrtc::{
    api::{
        APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_OPUS, MediaEngine},
    },
    ice_transport::{ice_candidate::RTCIceCandidateInit, ice_server::RTCIceServer},
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    track::track_local::{TrackLocal, track_local_static_sample::TrackLocalStaticSample},
};

pub(super) enum Command {
    Enable { id: String, ice: Vec<Value> },
    Members(Vec<Value>),
    Ice(Vec<Value>),
    Signal { from: String, signal: Value },
    Talk(bool),
    Mute { peer: String, muted: bool },
    Stop,
}
pub(super) enum Event {
    Send(Value),
    Ready,
    Error(String),
}
pub(super) struct Client {
    send: mpsc::SyncSender<Command>,
    receive: Mutex<mpsc::Receiver<Event>>,
    active: Arc<AtomicBool>,
    talking: Arc<AtomicBool>,
}
impl Client {
    pub fn start() -> Result<Self, String> {
        let (send, receive) = mpsc::sync_channel(64);
        let (events, output) = mpsc::sync_channel(128);
        let active = Arc::new(AtomicBool::new(true));
        let talking = Arc::new(AtomicBool::new(false));
        let alive = active.clone();
        let held = talking.clone();
        std::thread::Builder::new()
            .name("review-party-media".into())
            .spawn(move || {
                if let Err(error) = run(receive, events.clone(), alive, held) {
                    let _ = events.try_send(Event::Error(error));
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            send,
            receive: Mutex::new(output),
            active,
            talking,
        })
    }
    pub fn command(&self, command: Command) {
        if let Command::Talk(held) = command {
            self.talking.store(held, Ordering::Release);
        } else {
            let _ = self.send.try_send(command);
        }
    }
    pub fn events(&self) -> Vec<Event> {
        self.receive
            .lock()
            .map(|receiver| receiver.try_iter().take(128).collect())
            .unwrap_or_default()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.talking.store(false, Ordering::Release);
        self.active.store(false, Ordering::Release);
        let _ = self.send.try_send(Command::Stop);
    }
}

struct Peer {
    connection: Arc<RTCPeerConnection>,
    track: Arc<TrackLocalStaticSample>,
    pending: Vec<RTCIceCandidateInit>,
}
impl Peer {
    async fn new(
        id: &str,
        ice: Vec<RTCIceServer>,
        events: mpsc::SyncSender<Event>,
        samples: mpsc::SyncSender<(String, Vec<i16>)>,
    ) -> Result<Self, String> {
        let mut engine = MediaEngine::default();
        let capability = RTCRtpCodecCapability {
            mime_type: MIME_TYPE_OPUS.into(),
            clock_rate: 48_000,
            channels: 2,
            sdp_fmtp_line: "minptime=10;useinbandfec=1".into(),
            ..Default::default()
        };
        engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: capability.clone(),
                    payload_type: 111,
                    ..Default::default()
                },
                RTPCodecType::Audio,
            )
            .map_err(|error| error.to_string())?;
        let registry = register_default_interceptors(Registry::new(), &mut engine)
            .map_err(|error| error.to_string())?;
        let api = APIBuilder::new()
            .with_media_engine(engine)
            .with_interceptor_registry(registry)
            .build();
        let connection = Arc::new(
            api.new_peer_connection(RTCConfiguration {
                ice_servers: ice,
                ..Default::default()
            })
            .await
            .map_err(|error| error.to_string())?,
        );
        let recipient = id.to_owned();
        let health_events = events.clone();
        connection.on_peer_connection_state_change(Box::new(move |state| {
            let events = health_events.clone();
            Box::pin(async move {
                if state == RTCPeerConnectionState::Failed {
                    let _ = events.try_send(Event::Error("Peer voice connection failed. Check TURN configuration and enable voice again.".into()));
                }
            })
        }));
        connection.on_ice_candidate(Box::new(move |candidate| {
            let events = events.clone(); let recipient = recipient.clone();
            Box::pin(async move {
                let candidate = candidate.and_then(|candidate| candidate.to_json().ok()).and_then(|value| serde_json::to_value(value).ok());
                let _ = events.try_send(Event::Send(json!({"type":"signal","to":recipient,"signal":{"type":"ice","candidate":candidate}})));
            })
        }));
        let sender = id.to_owned();
        connection.on_track(Box::new(move |track, _, _| {
            let samples = samples.clone();
            let sender = sender.clone();
            Box::pin(async move {
                if !track
                    .codec()
                    .capability
                    .mime_type
                    .eq_ignore_ascii_case(MIME_TYPE_OPUS)
                {
                    return;
                }
                let Ok(mut playout) = opus_codec::Playout::new() else {
                    return;
                };
                loop {
                    match tokio::time::timeout(Duration::from_millis(10), track.read_rtp()).await {
                        Ok(Ok((packet, _))) => playout.push(
                            packet.header.sequence_number,
                            packet.payload.to_vec(),
                            std::time::Instant::now(),
                        ),
                        Ok(Err(_)) => break,
                        Err(_) => {}
                    }
                    for pcm in playout.drain(std::time::Instant::now()) {
                        let _ = samples.try_send((sender.clone(), pcm));
                    }
                }
            })
        }));
        let track = Arc::new(TrackLocalStaticSample::new(
            capability,
            "microphone".into(),
            "vibe-city-party".into(),
        ));
        let rtp = connection
            .add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|error| error.to_string())?;
        tokio::spawn(async move { while rtp.read_rtcp().await.is_ok() {} });
        Ok(Self {
            connection,
            track,
            pending: vec![],
        })
    }
    async fn offer(&self) -> Result<Value, String> {
        let offer = self
            .connection
            .create_offer(None)
            .await
            .map_err(|error| error.to_string())?;
        self.connection
            .set_local_description(offer.clone())
            .await
            .map_err(|error| error.to_string())?;
        Ok(json!({"type":"offer","sdp":offer.sdp}))
    }
    async fn signal(&mut self, value: Value) -> Result<Option<Value>, String> {
        match value["type"].as_str() {
            Some("ice") => {
                if value["candidate"].is_null() {
                    return Ok(None);
                }
                let candidate: RTCIceCandidateInit =
                    serde_json::from_value(value["candidate"].clone())
                        .map_err(|error| error.to_string())?;
                if self.connection.remote_description().await.is_some() {
                    self.connection
                        .add_ice_candidate(candidate)
                        .await
                        .map_err(|error| error.to_string())?;
                } else if self.pending.len() < 64 {
                    self.pending.push(candidate);
                }
                Ok(None)
            }
            Some(kind @ ("offer" | "answer")) => {
                let sdp = value["sdp"]
                    .as_str()
                    .ok_or("Invalid peer session description.")?
                    .to_owned();
                let description = if kind == "offer" {
                    RTCSessionDescription::offer(sdp)
                } else {
                    RTCSessionDescription::answer(sdp)
                }
                .map_err(|error| error.to_string())?;
                self.connection
                    .set_remote_description(description)
                    .await
                    .map_err(|error| error.to_string())?;
                for candidate in self.pending.drain(..) {
                    self.connection
                        .add_ice_candidate(candidate)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                if kind == "offer" {
                    let answer = self
                        .connection
                        .create_answer(None)
                        .await
                        .map_err(|error| error.to_string())?;
                    self.connection
                        .set_local_description(answer.clone())
                        .await
                        .map_err(|error| error.to_string())?;
                    Ok(Some(json!({"type":"answer","sdp":answer.sdp})))
                } else {
                    Ok(None)
                }
            }
            _ => Err("Unsupported room media signal.".into()),
        }
    }
}

fn ice_servers(values: Vec<Value>) -> Vec<RTCIceServer> {
    values
        .into_iter()
        .filter_map(|value| {
            let urls = if let Some(url) = value["urls"].as_str() {
                vec![url.to_owned()]
            } else {
                value["urls"]
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            };
            Some(RTCIceServer {
                urls,
                username: value["username"].as_str().unwrap_or("").into(),
                credential: value["credential"].as_str().unwrap_or("").into(),
            })
        })
        .collect()
}

fn run(
    commands: mpsc::Receiver<Command>,
    events: mpsc::SyncSender<Event>,
    active: Arc<AtomicBool>,
    talking: Arc<AtomicBool>,
) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let (samples, incoming) = mpsc::sync_channel(128);
    let mut id = String::new();
    let mut ice = vec![];
    let mut peers: HashMap<String, Peer> = HashMap::new();
    let mut muted = HashSet::new();
    let mut allowed = HashSet::new();
    let mut playback: HashMap<String, std::collections::VecDeque<Vec<i16>>> = HashMap::new();
    let mut next_playback = std::time::Instant::now();
    let gate = Arc::new(AtomicBool::new(false));
    let mut opening = None::<audio::Opening>;
    let mut devices = None::<audio::Devices>;
    let mut encoder = opus_codec::Encoder::new()?;
    while active.load(Ordering::Acquire) {
        match commands.recv_timeout(Duration::from_millis(5)) {
            Err(mpsc::RecvTimeoutError::Disconnected) | Ok(Command::Stop) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(Command::Enable {
                id: local,
                ice: servers,
            }) => {
                id = local;
                ice = ice_servers(servers);
                opening = Some(audio::Opening::start(gate.clone()));
            }
            Ok(Command::Ice(servers)) => {
                ice = ice_servers(servers);
                for peer in peers.values() {
                    let mut configuration = runtime.block_on(peer.connection.get_configuration());
                    configuration.ice_servers = ice.clone();
                    runtime
                        .block_on(peer.connection.set_configuration(configuration))
                        .map_err(|error| error.to_string())?;
                }
            }
            Ok(Command::Talk(held)) => talking.store(held, Ordering::Release),
            Ok(Command::Mute { peer, muted: value }) => {
                if value {
                    muted.insert(peer);
                } else {
                    muted.remove(&peer);
                }
            }
            Ok(Command::Members(members)) => {
                allowed = members
                    .iter()
                    .filter(|member| member["voice"]["enabled"] == true)
                    .filter_map(|member| member["id"].as_str())
                    .filter(|peer| *peer != id)
                    .map(str::to_owned)
                    .collect();
                let removed = peers
                    .keys()
                    .filter(|peer| !allowed.contains(*peer))
                    .cloned()
                    .collect::<Vec<_>>();
                for peer in removed {
                    if let Some(peer) = peers.remove(&peer) {
                        let _ = runtime.block_on(peer.connection.close());
                    }
                }
                for peer in &allowed {
                    if peers.contains_key(peer) || devices.is_none() || id.as_str() > peer.as_str()
                    {
                        continue;
                    }
                    let connection = runtime.block_on(Peer::new(
                        peer,
                        ice.clone(),
                        events.clone(),
                        samples.clone(),
                    ))?;
                    let offer = runtime.block_on(connection.offer())?;
                    let _ = events.try_send(Event::Send(
                        json!({"type":"signal","to":peer,"signal":offer}),
                    ));
                    peers.insert(peer.clone(), connection);
                }
            }
            Ok(Command::Signal { from, signal }) => {
                if !allowed.contains(&from) || devices.is_none() {
                    continue;
                }
                if !peers.contains_key(&from) {
                    let connection = runtime.block_on(Peer::new(
                        &from,
                        ice.clone(),
                        events.clone(),
                        samples.clone(),
                    ))?;
                    peers.insert(from.clone(), connection);
                }
                if let Some(peer) = peers.get_mut(&from) {
                    if let Some(answer) = runtime.block_on(peer.signal(signal))? {
                        let _ = events.try_send(Event::Send(
                            json!({"type":"signal","to":from,"signal":answer}),
                        ));
                    }
                }
            }
        }
        if let Some(result) = opening.as_mut().and_then(audio::Opening::poll) {
            opening = None;
            devices = Some(result?);
            let _ = events.try_send(Event::Ready);
        }
        if let Some(devices) = devices.as_mut() {
            if !devices.healthy() {
                return Err("Room audio device stopped. Disable and enable voice to retry.".into());
            }
            while let Some(frame) = devices.capture() {
                if !talking.load(Ordering::Acquire) || !active.load(Ordering::Acquire) {
                    continue;
                }
                let encoded = encoder.encode(&frame)?;
                for peer in peers.values() {
                    let sample = Sample {
                        data: encoded.clone().into(),
                        duration: Duration::from_millis(20),
                        ..Default::default()
                    };
                    runtime
                        .block_on(peer.track.write_sample(&sample))
                        .map_err(|error| error.to_string())?;
                }
            }
            // Use decoded remote tracks directly; never recapture speaker output.
            // Party input is not routed to Codex until an independently consented adapter exists.
            for (peer, pcm) in incoming.try_iter().take(128) {
                if !muted.contains(&peer) && allowed.contains(&peer) {
                    let frames = playback.entry(peer).or_default();
                    if frames.len() >= 8 {
                        frames.pop_front();
                    }
                    frames.push_back(pcm);
                }
            }
            playback.retain(|peer, _| allowed.contains(peer) && !muted.contains(peer));
            if std::time::Instant::now() >= next_playback {
                next_playback = std::time::Instant::now() + Duration::from_millis(20);
                let frames = playback
                    .values_mut()
                    .filter_map(std::collections::VecDeque::pop_front)
                    .collect::<Vec<_>>();
                if !frames.is_empty() {
                    let mut mixed = vec![0i32; opus_codec::FRAME_SAMPLES];
                    for frame in &frames {
                        for (sample, value) in mixed.iter_mut().zip(frame) {
                            *sample += i32::from(*value);
                        }
                    }
                    let count = frames.len() as i32;
                    devices.append(
                        &mixed
                            .into_iter()
                            .map(|sample| {
                                (sample / count).clamp(i16::MIN as i32, i16::MAX as i32) as i16
                            })
                            .collect::<Vec<_>>(),
                    );
                }
            }
        }
    }
    talking.store(false, Ordering::Release);
    gate.store(true, Ordering::Release);
    for peer in peers.into_values() {
        let _ = runtime.block_on(peer.connection.close());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn releasing_push_to_talk_is_immediate_even_when_signaling_queue_is_full() {
        let (send, _commands) = mpsc::sync_channel(1);
        assert!(send.send(Command::Stop).is_ok());
        let (_events, receive) = mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(true));
        let talking = Arc::new(AtomicBool::new(true));
        let client = Client {
            send,
            receive: Mutex::new(receive),
            active: active.clone(),
            talking: talking.clone(),
        };
        client.command(Command::Talk(false));
        assert!(!talking.load(Ordering::Acquire));
        client.command(Command::Talk(true));
        drop(client);
        assert!(!talking.load(Ordering::Acquire));
        assert!(!active.load(Ordering::Acquire));
    }
    #[test]
    fn ice_configuration_preserves_turn_credentials_and_normalizes_urls() {
        let servers = ice_servers(vec![
            json!({"urls":"turn:relay.test:3478","username":"lease-user","credential":"lease-secret"}),
            json!({"urls":["stun:stun.test"]}),
        ]);
        assert_eq!(servers[0].urls, vec!["turn:relay.test:3478"]);
        assert_eq!(servers[0].username, "lease-user");
        assert_eq!(servers[0].credential, "lease-secret");
        assert_eq!(servers[1].urls, vec!["stun:stun.test"]);
    }
}
