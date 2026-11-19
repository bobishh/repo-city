//! Native WebRTC media for the ChatGPT-authenticated app-server voice transport.
use super::opus_codec::{self, Encoder, Playout};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};
use tokio::runtime::Runtime;
use webrtc::{
    api::{
        APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_OPUS, MediaEngine},
    },
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::{
        RTCRtpCodecCapability, RTCRtpCodecParameters, RTCRtpHeaderExtensionCapability, RTPCodecType,
    },
    track::track_local::{TrackLocal, track_local_static_sample::TrackLocalStaticSample},
};

#[derive(Default)]
struct Metrics {
    inbound_packets: AtomicU64,
    inbound_samples: AtomicU64,
    concealed_frames: AtomicU64,
    dropped_packets: AtomicU64,
    invalid_packets: AtomicU64,
    ended_tracks: AtomicU64,
    outbound_frames: AtomicU64,
    outbound_samples: AtomicU64,
    outbound_peak: AtomicU32,
}

pub(super) struct Connection {
    runtime: Option<Runtime>,
    peer: Arc<RTCPeerConnection>,
    track: Arc<TrackLocalStaticSample>,
    output: Receiver<Result<Vec<i16>, String>>,
    encoder: Mutex<Encoder>,
    connected: Arc<AtomicBool>,
    events_open: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    sdp: String,
    metrics: Arc<Metrics>,
}

impl Connection {
    /// Gather a native audio/data offer without opening any audio device.
    pub fn offer() -> Result<Self, String> {
        let encoder = Mutex::new(Encoder::new()?);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| format!("Could not start native voice runtime: {error}"))?;
        let connected = Arc::new(AtomicBool::new(false));
        let events_open = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let metrics = Arc::new(Metrics::default());
        let (send, output) = mpsc::sync_channel(128);
        let (peer, track, sdp) = runtime.block_on(async {
            let mut engine = MediaEngine::default();
            let codec = capability();
            engine
                .register_codec(
                    RTCRtpCodecParameters {
                        capability: codec.clone(),
                        payload_type: 111,
                        ..Default::default()
                    },
                    RTPCodecType::Audio,
                )
                .map_err(|error| format!("Native voice codec: {error}"))?;
            register_audio_extensions(&mut engine)?;
            let registry = register_default_interceptors(Registry::new(), &mut engine)
                .map_err(|error| format!("Native voice RTP: {error}"))?;
            let api = APIBuilder::new()
                .with_media_engine(engine)
                .with_interceptor_registry(registry)
                .build();
            // As in Ecky's browser peer, use the service's public ICE candidates.
            // No third-party STUN service or browser process is started here.
            let peer = Arc::new(
                api.new_peer_connection(RTCConfiguration::default())
                    .await
                    .map_err(|error| format!("Native voice peer: {error}"))?,
            );
            let state = connected.clone();
            let dead = failed.clone();
            peer.on_peer_connection_state_change(Box::new(move |value| {
                let state = state.clone();
                let dead = dead.clone();
                Box::pin(async move {
                    state.store(
                        value == RTCPeerConnectionState::Connected,
                        Ordering::Release,
                    );
                    if matches!(
                        value,
                        RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed
                    ) {
                        dead.store(true, Ordering::Release);
                    }
                })
            }));
            let received = metrics.clone();
            peer.on_track(Box::new(move |track, _, _| {
                let send = send.clone();
                let received = received.clone();
                Box::pin(async move {
                    let mut playout = match Playout::new() {
                        Ok(playout) => playout,
                        Err(error) => {
                            let _ = send.try_send(Err(error));
                            return;
                        }
                    };
                    let mut counted = (0, 0, 0);
                    loop {
                        match tokio::time::timeout(Duration::from_millis(10), track.read_rtp())
                            .await
                        {
                            Ok(Ok((packet, _))) => {
                                if packet.header.payload_type != 111 {
                                    received.invalid_packets.fetch_add(1, Ordering::Relaxed);
                                    continue;
                                }
                                received.inbound_packets.fetch_add(1, Ordering::Relaxed);
                                playout.push(
                                    packet.header.sequence_number,
                                    packet.payload.to_vec(),
                                    std::time::Instant::now(),
                                );
                            }
                            Ok(Err(_)) => {
                                received.ended_tracks.fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                            Err(_) => {} // DTX and jitter are ordinary; keep the call alive.
                        }
                        for samples in playout.drain(std::time::Instant::now()) {
                            received
                                .inbound_samples
                                .fetch_add(samples.len() as u64, Ordering::Relaxed);
                            if send.try_send(Ok(samples)).is_err() {
                                received.dropped_packets.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        received
                            .concealed_frames
                            .fetch_add(playout.concealed - counted.0, Ordering::Relaxed);
                        received
                            .dropped_packets
                            .fetch_add(playout.dropped - counted.1, Ordering::Relaxed);
                        received
                            .invalid_packets
                            .fetch_add(playout.invalid - counted.2, Ordering::Relaxed);
                        counted = (playout.concealed, playout.dropped, playout.invalid);
                    }
                })
            }));
            let track = Arc::new(TrackLocalStaticSample::new(
                codec,
                "microphone".into(),
                "vibe-city".into(),
            ));
            let sender = peer
                .add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .map_err(|error| format!("Native voice audio track: {error}"))?;
            tokio::spawn(async move { while sender.read_rtcp().await.is_ok() {} });
            // Codex requires this ordered channel in its SDP even though app-server
            // owns the authenticated transcript/delegation sideband.
            let channel = peer
                .create_data_channel("oai-events", None)
                .await
                .map_err(|error| format!("Native voice events: {error}"))?;
            let ready = events_open.clone();
            channel.on_open(Box::new(move || {
                Box::pin(async move {
                    ready.store(true, Ordering::Release);
                })
            }));
            let offer = peer
                .create_offer(None)
                .await
                .map_err(|error| format!("Native voice offer: {error}"))?;
            let mut gathered = peer.gathering_complete_promise().await;
            peer.set_local_description(offer)
                .await
                .map_err(|_| "Could not configure the native voice offer.".to_owned())?;
            if tokio::time::timeout(Duration::from_secs(12), gathered.recv())
                .await
                .is_err()
            {
                let _ = peer.close().await;
                return Err("Native voice ICE gathering timed out.".into());
            }
            let sdp = peer
                .local_description()
                .await
                .ok_or("Native voice did not produce an SDP offer.")?
                .sdp;
            Ok::<_, String>((peer, track, sdp))
        })?;
        Ok(Self {
            runtime: Some(runtime),
            peer,
            track,
            output,
            encoder,
            connected,
            events_open,
            failed,
            sdp,
            metrics,
        })
    }

    pub fn sdp(&self) -> &str {
        &self.sdp
    }

    pub fn answer(&self, sdp: String) -> Result<(), String> {
        if !opus_answer(&sdp) {
            return Err("The voice service did not accept Opus 48 kHz audio.".into());
        }
        let answer = RTCSessionDescription::answer(sdp)
            .map_err(|_| "The voice service returned an invalid SDP answer.".to_owned())?;
        self.runtime
            .as_ref()
            .ok_or("Native voice runtime closed.")?
            .block_on(async {
                self.peer
                    .set_remote_description(answer)
                    .await
                    .map_err(|_| "Could not apply the native voice SDP answer.".to_owned())
            })
    }

    pub fn ready(&self) -> bool {
        self.connected.load(Ordering::Acquire) && self.events_open.load(Ordering::Acquire)
    }
    pub fn healthy(&self) -> bool {
        !self.failed.load(Ordering::Acquire)
    }

    pub fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({"connection_state":self.peer.connection_state().to_string(),"ice_state":self.peer.ice_connection_state().to_string(),"events_open":self.events_open.load(Ordering::Acquire),"failed":self.failed.load(Ordering::Acquire),"dtls_state":format!("{:?}",self.peer.sctp().transport().state()),"sctp_state":format!("{:?}",self.peer.sctp().state()),"inbound_packets":self.metrics.inbound_packets.load(Ordering::Relaxed),"inbound_samples_48k":self.metrics.inbound_samples.load(Ordering::Relaxed),"codec":"opus/48000/2","concealed_frames":self.metrics.concealed_frames.load(Ordering::Relaxed),"dropped_packets":self.metrics.dropped_packets.load(Ordering::Relaxed),"invalid_packets":self.metrics.invalid_packets.load(Ordering::Relaxed),"ended_tracks":self.metrics.ended_tracks.load(Ordering::Relaxed),"outbound_frames":self.metrics.outbound_frames.load(Ordering::Relaxed),"outbound_samples_48k":self.metrics.outbound_samples.load(Ordering::Relaxed),"outbound_peak":f32::from_bits(self.metrics.outbound_peak.swap(0,Ordering::Relaxed))})
    }

    pub fn send(&self, samples: &[i16]) -> Result<(), String> {
        let payload = self
            .encoder
            .lock()
            .map_err(|_| "Native Opus encoder unavailable.")?
            .encode(samples)?;
        if payload.is_empty() {
            return Ok(());
        }
        let duration = Duration::from_millis(20);
        self.runtime
            .as_ref()
            .ok_or("Native voice runtime closed.")?
            .block_on(async {
                tokio::time::timeout(
                    Duration::from_millis(500),
                    self.track.write_sample(&Sample {
                        data: payload.into(),
                        duration,
                        ..Default::default()
                    }),
                )
                .await
                .map_err(|_| "Native voice audio send timed out.".to_owned())?
                .map_err(|error| format!("Native voice audio send: {error}"))
            })?;
        self.metrics.outbound_frames.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .outbound_samples
            .fetch_add(samples.len() as u64, Ordering::Relaxed);
        let peak = samples
            .iter()
            .map(|sample| sample.unsigned_abs())
            .max()
            .unwrap_or(0) as f32
            / 32768.;
        self.metrics
            .outbound_peak
            .fetch_max(peak.to_bits(), Ordering::Relaxed);
        Ok(())
    }

    pub fn receive(&self) -> Result<Vec<Vec<i16>>, String> {
        self.output.try_iter().take(64).collect()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            let _ = runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(2), self.peer.close()).await
            });
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

fn register_audio_extensions(engine: &mut MediaEngine) -> Result<(), String> {
    // The service may start another RTP SSRC. webrtc-rs requires negotiated
    // MID/RID metadata to route undeclared streams when audio and data coexist.
    for uri in [
        "urn:ietf:params:rtp-hdrext:sdes:mid",
        "urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id",
        "urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id",
    ] {
        engine
            .register_header_extension(
                RTCRtpHeaderExtensionCapability { uri: uri.into() },
                RTPCodecType::Audio,
                None,
            )
            .map_err(|error| format!("Native voice RTP extension: {error}"))?;
    }
    Ok(())
}

fn capability() -> RTCRtpCodecCapability {
    RTCRtpCodecCapability {
        mime_type: MIME_TYPE_OPUS.into(),
        clock_rate: opus_codec::SAMPLE_RATE,
        channels: 2,
        sdp_fmtp_line: "minptime=10;useinbandfec=1;stereo=0;sprop-stereo=0".into(),
        ..Default::default()
    }
}

fn opus_answer(sdp: &str) -> bool {
    sdp.len() <= 65536
        && sdp.lines().any(|line| {
            let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
            fields.len() >= 4
                && fields[0] == "m=audio"
                && fields[1] != "0"
                && fields[3..].contains(&"111")
        })
        && sdp
            .to_ascii_lowercase()
            .contains("a=rtpmap:111 opus/48000/2")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdp_requires_negotiated_non_rejected_opus_audio() {
        assert!(opus_answer(
            "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n"
        ));
        assert!(!opus_answer(
            "m=audio 0 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n"
        ));
        assert!(!opus_answer(
            "m=audio 9 UDP/TLS/RTP/SAVPF 0\r\na=rtpmap:0 PCMU/8000\r\n"
        ));
    }

    #[test]
    fn native_offer_contains_opus_and_events_without_opening_microphone() {
        let connection = Connection::offer().unwrap();
        assert!(connection.sdp().contains("a=rtpmap:111 opus/48000/2"));
        assert!(connection.sdp().contains("m=application"));
        assert!(
            connection
                .sdp()
                .contains("urn:ietf:params:rtp-hdrext:sdes:mid")
        );
        assert!(
            connection
                .sdp()
                .contains("urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id")
        );
        assert!(!connection.ready());
    }

    #[test]
    fn native_peer_negotiates_ordered_events_with_a_local_native_answerer() {
        let connection = Connection::offer().unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let (sent, received_by_answerer) = mpsc::sync_channel(16);
        let (peer, answer, source) = runtime.block_on(async {
            let mut engine = MediaEngine::default();
            engine
                .register_codec(
                    RTCRtpCodecParameters {
                        capability: capability(),
                        payload_type: 111,
                        ..Default::default()
                    },
                    RTPCodecType::Audio,
                )
                .unwrap();
            register_audio_extensions(&mut engine).unwrap();
            let api = APIBuilder::new().with_media_engine(engine).build();
            let peer = api
                .new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap();
            peer.on_track(Box::new(move |track, _, _| {
                let sent = sent.clone();
                Box::pin(async move {
                    for _ in 0..8 {
                        if let Ok((packet, _)) = track.read_rtp().await {
                            let _ = sent.try_send(packet.payload.to_vec());
                        }
                    }
                })
            }));
            let source = Arc::new(TrackLocalStaticSample::new(
                capability(),
                "fixture".into(),
                "fixture".into(),
            ));
            let sender = peer
                .add_track(source.clone() as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .unwrap();
            tokio::spawn(async move { while sender.read_rtcp().await.is_ok() {} });
            peer.set_remote_description(
                RTCSessionDescription::offer(connection.sdp().into()).unwrap(),
            )
            .await
            .unwrap();
            let answer = peer.create_answer(None).await.unwrap();
            let mut gathered = peer.gathering_complete_promise().await;
            peer.set_local_description(answer).await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), gathered.recv())
                .await
                .unwrap();
            let answer = peer.local_description().await.unwrap().sdp;
            (peer, answer, source)
        });
        connection.answer(answer).unwrap();
        let started = std::time::Instant::now();
        while !connection.ready()
            && connection.healthy()
            && started.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let ready = connection.ready();
        let diagnostics = connection.diagnostics();
        let waveform = (0..opus_codec::FRAME_SAMPLES)
            .map(|index| {
                (10_000.
                    * (index as f64 * 6500. * std::f64::consts::TAU
                        / opus_codec::SAMPLE_RATE as f64)
                        .sin()) as i16
            })
            .collect::<Vec<_>>();
        let mut outbound_decoder = opus_codec::Decoder::new().unwrap();
        for _ in 0..8 {
            connection.send(&waveform).unwrap();
            let packet = received_by_answerer
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            let samples = outbound_decoder.decode(&packet).unwrap();
            assert_eq!(samples.len(), 960);
            assert!(samples.iter().any(|x| x.abs() > 1000));
        }
        let mut encoder = Encoder::new().unwrap();
        for frame in 0..8 {
            runtime
                .block_on(source.write_sample(&Sample {
                    data: encoder.encode(&waveform).unwrap().into(),
                    duration: Duration::from_millis(20),
                    // Simulate a real lost RTP sequence number. The receiver must use
                    // Opus FEC/PLC and keep the peer alive for the following frames.
                    prev_dropped_packets: u16::from(frame == 4),
                    ..Default::default()
                }))
                .unwrap();
        }
        let started = std::time::Instant::now();
        let mut received = Vec::new();
        while received.len() < 9 && started.elapsed() < Duration::from_secs(5) {
            received.extend(connection.receive().unwrap());
            if received.len() < 9 {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        assert_eq!(
            received.len(),
            9,
            "packets after recovery: {}",
            connection.diagnostics()
        );
        assert_eq!(connection.diagnostics()["concealed_frames"], 1);
        assert!(connection.healthy());
        runtime.block_on(peer.close()).unwrap();
        assert!(ready, "native loopback state: {diagnostics}");
        assert_eq!(received.first().map(Vec::len), Some(960));
        assert!(received.iter().all(|frame| frame.len() == 960));
        assert!(received[7].iter().any(|x| x.abs() > 1000));
    }
}
