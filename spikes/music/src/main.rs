// Song-search spike (throwaway).
//   music-spike <raw s16le mono 11025 file> [start_s] [len_s]  -> "duration fingerprint ms"
//   music-spike rec <secs>   records what the PC plays (Linux: default sink's monitor over the
//                            PulseAudio protocol; Windows: WASAPI loopback via cpal), same output.
use base64::Engine;
use rusty_chromaprint::{Configuration, FingerprintCompressor, Fingerprinter};

const RATE: usize = 11025;

fn fingerprint(clip: &[i16], rate: u32, channels: u32) -> (usize, String, u128) {
    let t = std::time::Instant::now();
    let cfg = Configuration::preset_test2();
    let mut p = Fingerprinter::new(&cfg);
    p.start(rate, channels).unwrap();
    p.consume(clip);
    p.finish();
    let fp = FingerprintCompressor::from(&cfg).compress(p.fingerprint());
    let enc = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(fp);
    let secs = clip.len() / (rate * channels) as usize;
    (secs, enc, t.elapsed().as_millis())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[1] == "rec" {
        let secs: f64 = a[2].parse().unwrap();
        let t = std::time::Instant::now();
        let (s, rate, ch) = rec(secs);
        let peak = s.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
        eprintln!(
            "recorded {} samples, rate {rate}, ch {ch}, peak {peak}, {} ms",
            s.len(),
            t.elapsed().as_millis()
        );
        let (d, e, ms) = fingerprint(&s, rate, ch);
        println!("{d} {e} {ms}");
        return;
    }
    let raw = std::fs::read(&a[1]).unwrap();
    let s: Vec<i16> = raw
        .as_chunks()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c))
        .collect();
    let start = a.get(2).map(|v| v.parse::<f64>().unwrap()).unwrap_or(0.0);
    let len = a.get(3).map(|v| v.parse::<f64>().unwrap()).unwrap_or(1e9);
    let i0 = ((start * RATE as f64) as usize).min(s.len());
    let i1 = (i0 + (len * RATE as f64) as usize).min(s.len());
    let (d, e, ms) = fingerprint(&s[i0..i1], RATE as u32, 1);
    println!("{d} {e} {ms}");
}

#[cfg(target_os = "linux")]
fn rec(secs: f64) -> (Vec<i16>, u32, u32) {
    use pulseaudio::protocol;
    use std::io::Read;
    let path = pulseaudio::socket_path_from_env().expect("no PulseAudio socket");
    let mut sock = std::io::BufReader::new(std::os::unix::net::UnixStream::connect(path).unwrap());
    let cookie = pulseaudio::cookie_path_from_env()
        .and_then(|p| std::fs::read(p).ok())
        .unwrap_or_default();
    let auth = protocol::AuthParams {
        version: protocol::MAX_VERSION,
        supports_shm: false,
        supports_memfd: false,
        cookie,
    };
    protocol::write_command_message(
        sock.get_mut(),
        0,
        &protocol::Command::Auth(auth),
        protocol::MAX_VERSION,
    )
    .unwrap();
    let (_, reply) =
        protocol::read_reply_message::<protocol::AuthReply>(&mut sock, protocol::MAX_VERSION)
            .unwrap();
    let v = protocol::MAX_VERSION.min(reply.version);
    let mut props = protocol::Props::new();
    props.set(protocol::Prop::ApplicationName, c"OpenCircle");
    protocol::write_command_message(
        sock.get_mut(),
        1,
        &protocol::Command::SetClientName(props),
        v,
    )
    .unwrap();
    protocol::read_reply_message::<protocol::SetClientNameReply>(&mut sock, v).unwrap();
    protocol::write_command_message(
        sock.get_mut(),
        2,
        &protocol::Command::CreateRecordStream(protocol::RecordStreamParams {
            source_name: Some(c"@DEFAULT_MONITOR@".to_owned()),
            sample_spec: protocol::SampleSpec {
                format: protocol::SampleFormat::S16Le,
                channels: 1,
                sample_rate: RATE as u32,
            },
            channel_map: protocol::ChannelMap::mono(),
            cvolume: Some(protocol::ChannelVolume::norm(1)),
            ..Default::default()
        }),
        v,
    )
    .unwrap();
    let (_, stream) =
        protocol::read_reply_message::<protocol::CreateRecordStreamReply>(&mut sock, v).unwrap();
    eprintln!("stream spec {:?}", stream.sample_spec);
    let want = (secs * RATE as f64) as usize * 2;
    let mut bytes = Vec::with_capacity(want);
    let mut buf = Vec::new();
    while bytes.len() < want {
        let desc = protocol::read_descriptor(&mut sock).unwrap();
        if desc.channel == u32::MAX {
            let _ = protocol::Command::read_tag_prefixed(&mut sock, v);
        } else {
            buf.resize(desc.length as usize, 0);
            sock.read_exact(&mut buf).unwrap();
            bytes.extend_from_slice(&buf);
        }
    }
    bytes.truncate(want);
    let s = bytes
        .as_chunks()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c))
        .collect();
    (s, RATE as u32, 1)
}

#[cfg(windows)]
fn rec(secs: f64) -> (Vec<i16>, u32, u32) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::{Arc, Mutex};
    let dev = cpal::default_host()
        .default_output_device()
        .expect("no output device");
    let cfg = dev.default_output_config().unwrap();
    eprintln!("device {dev}, config {cfg:?}");
    let rate = cfg.sample_rate();
    let ch = cfg.channels() as u32;
    let out = Arc::new(Mutex::new(Vec::<i16>::new()));
    let o = out.clone();
    // Input stream on an output device = WASAPI loopback.
    let stream = dev
        .build_input_stream(
            cfg.config(),
            move |d: &[f32], _: &cpal::InputCallbackInfo| {
                o.lock()
                    .unwrap()
                    .extend(d.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16))
            },
            |e| eprintln!("stream error {e}"),
            None,
        )
        .unwrap();
    stream.play().unwrap();
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    drop(stream);
    let s = std::mem::take(&mut *out.lock().unwrap());
    (s, rate, ch)
}
