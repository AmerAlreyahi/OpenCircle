//! The walkthrough as a video (v0.4.3): AV1 by rav1e (BSD-2, pure Rust, no
//! asm) in an MP4 or a WebM file, each written by the small muxer here
//! (one video track, one sample a frame; no crate: the two layouts are a page
//! each). Frames come in already the same size, even width and height.
use rav1e::prelude::*;

use crate::shared::picture::Crop;

/// A frame's time and the encoder's choices. Quantizer 100 of 255: text on a
/// screenshot stays sharp, a flat screen costs little.
const QUANTIZER: usize = 100;

/// One frame's data, whether it is a key frame (a place a player can seek to) and
/// how long it shows (ms): a held step is ONE sample with its whole time.
pub struct Sample {
    pub data: Vec<u8>,
    pub key: bool,
    pub ms: u32,
}

/// The `av1C` record and one sample a frame.
pub type Encoded = (Vec<u8>, Vec<Sample>);

/// A key frame at least this often (frames); a walkthrough is far shorter, so
/// one key frame at its start.
pub const KEY_EVERY: u64 = 240;

/// `frames` (`total` of them, each with the time it shows in ms; same size, even
/// sides) as AV1: the `av1C` record and one sample each. Frames are taken one at
/// a time (a mixed frame lives only until the encoder has it). `progress(done,
/// total)` after every sample: false = cancel (`None`).
pub fn encode_av1<'a>(
    mut frames: impl Iterator<Item = (std::borrow::Cow<'a, Crop>, u32)>,
    total: usize,
    key_every: u64,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<Encoded>, String> {
    let Some((first, first_ms)) = frames.next() else {
        return Err("no frames".into());
    };
    let (w, h) = (first.width as usize, first.height as usize);
    let mut enc = EncoderConfig::with_speed_preset(10);
    enc.width = w;
    enc.height = h;
    // (the file's own sample times are what players use; this is only the encoder's clock)
    enc.time_base = Rational::new(50, 1000);
    enc.low_latency = true;
    enc.min_key_frame_interval = key_every;
    enc.max_key_frame_interval = key_every;
    enc.quantizer = QUANTIZER;
    enc.color_description = Some(ColorDescription {
        color_primaries: ColorPrimaries::BT709,
        transfer_characteristics: TransferCharacteristics::BT709,
        matrix_coefficients: MatrixCoefficients::BT709,
    });
    // about half the cores: the app stays quick while a video is made
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get() / 2)
        .max(1);
    let cfg = Config::new().with_encoder_config(enc).with_threads(threads);
    let mut ctx: Context<u8> = cfg.new_context().map_err(|e| e.to_string())?;
    let mut samples = Vec::new();
    // each frame's time, in the order they went in (packets come out in order)
    let mut times = std::collections::VecDeque::new();
    let mut take = |ctx: &mut Context<u8>,
                    samples: &mut Vec<Sample>,
                    times: &mut std::collections::VecDeque<u32>|
     -> Result<bool, String> {
        loop {
            match ctx.receive_packet() {
                Ok(p) => {
                    samples.push(Sample {
                        key: p.frame_type == FrameType::KEY,
                        data: strip_delimiter(p.data),
                        ms: times.pop_front().unwrap_or(0),
                    });
                    if !progress(samples.len(), total) {
                        return Ok(false);
                    }
                }
                Err(EncoderStatus::Encoded) => {}
                Err(EncoderStatus::NeedMoreData | EncoderStatus::LimitReached) => return Ok(true),
                Err(e) => return Err(e.to_string()),
            }
        }
    };
    for (f, ms) in std::iter::once((first, first_ms)).chain(frames) {
        let mut frame = ctx.new_frame();
        let (y, u, v) = yuv420(&f);
        drop(f); // (a mixed frame is gone as soon as the encoder has it)
        frame.planes[0].copy_from_raw_u8(&y, w, 1);
        frame.planes[1].copy_from_raw_u8(&u, w / 2, 1);
        frame.planes[2].copy_from_raw_u8(&v, w / 2, 1);
        times.push_back(ms);
        ctx.send_frame(frame).map_err(|e| e.to_string())?;
        if !take(&mut ctx, &mut samples, &mut times)? {
            return Ok(None);
        }
    }
    ctx.flush();
    if !take(&mut ctx, &mut samples, &mut times)? {
        return Ok(None);
    }
    if samples.len() != total {
        return Err(format!("{} of {total} frames came out", samples.len()));
    }
    Ok(Some((ctx.container_sequence_header(), samples)))
}

/// A sample without its temporal delimiter (MP4 and Matroska both leave it out).
fn strip_delimiter(data: Vec<u8>) -> Vec<u8> {
    match data.starts_with(&[0x12, 0x00]) {
        true => data[2..].to_vec(),
        false => data,
    }
}

/// BT.709, limited range, 4:2:0 (chroma averaged over each 2x2).
fn yuv420(f: &Crop) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (w, h) = (f.width as usize, f.height as usize);
    let px = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        [f.rgba[i] as f32, f.rgba[i + 1] as f32, f.rgba[i + 2] as f32]
    };
    let mut yy = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = px(x, y);
            let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            yy.push((16.0 + l * 219.0 / 255.0).round() as u8);
        }
    }
    let (mut u, mut v) = (Vec::with_capacity(w * h / 4), Vec::with_capacity(w * h / 4));
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let mut s = [0.0f32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = px(x + dx, y + dy);
                s.iter_mut().zip(p).for_each(|(s, p)| *s += p / 4.0);
            }
            let l = 0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2];
            let cb = (s[2] - l) / 1.8556;
            let cr = (s[0] - l) / 1.5748;
            u.push((128.0 + cb * 224.0 / 255.0).round().clamp(16.0, 240.0) as u8);
            v.push((128.0 + cr * 224.0 / 255.0).round().clamp(16.0, 240.0) as u8);
        }
    }
    (yy, u, v)
}

/// The identity matrix of a movie / track header.
const UNITY: [u8; 36] = [
    0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0x40, 0, 0, 0,
];

fn be32(n: u32) -> [u8; 4] {
    n.to_be_bytes()
}

/// A box: size, name, body.
fn mp4_box(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = be32(body.len() as u32 + 8).to_vec();
    out.extend_from_slice(name);
    out.extend_from_slice(body);
    out
}

/// A full box (version 0): flags, then the body.
fn full(name: &[u8; 4], flags: u32, body: &[u8]) -> Vec<u8> {
    let mut b = be32(flags).to_vec();
    b.extend_from_slice(body);
    mp4_box(name, &b)
}

/// The samples in an MP4 (ISO BMFF): `ftyp`, `moov` (one `av01` track; the
/// time unit is a millisecond) and one `mdat` chunk.
pub fn mp4(w: u32, h: u32, av1c: &[u8], samples: &[Sample]) -> Result<Vec<u8>, String> {
    let too_big = || "the video is too big for an MP4".to_string();
    let n = u32::try_from(samples.len()).map_err(|_| too_big())?;
    let total = (samples.iter())
        .try_fold(0u32, |t, x| t.checked_add(x.ms))
        .ok_or_else(too_big)?;
    let (w16, h16) = (
        u16::try_from(w).map_err(|_| too_big())?,
        u16::try_from(h).map_err(|_| too_big())?,
    );
    let (w_fixed, h_fixed) = (
        w.checked_shl(16)
            .filter(|_| w < 1 << 16)
            .ok_or_else(too_big)?,
        h.checked_shl(16)
            .filter(|_| h < 1 << 16)
            .ok_or_else(too_big)?,
    );
    let ftyp = mp4_box(
        b"ftyp",
        &[b"isom".as_slice(), &be32(0), b"isomiso2av01mp41"].concat(),
    );
    let moov = |chunk_at: u32| {
        let mvhd = full(
            b"mvhd",
            0,
            &[
                &be32(0)[..],
                &be32(0),
                &be32(1000),
                &be32(total),
                &be32(0x0001_0000),
                &[0x01, 0],
                &[0; 10],
                &UNITY,
                &[0; 24],
                &be32(2),
            ]
            .concat(),
        );
        let tkhd = full(
            b"tkhd",
            3,
            &[
                &be32(0)[..],
                &be32(0),
                &be32(1),
                &be32(0),
                &be32(total),
                &[0; 8],
                &[0; 4],
                &[0, 0, 0, 0],
                &UNITY,
                &be32(w_fixed),
                &be32(h_fixed),
            ]
            .concat(),
        );
        let mdhd = full(
            b"mdhd",
            0,
            &[
                &be32(0)[..],
                &be32(0),
                &be32(1000),
                &be32(total),
                &[0x55, 0xc4, 0, 0],
            ]
            .concat(),
        );
        let hdlr = full(
            b"hdlr",
            0,
            &[&be32(0)[..], b"vide", &[0; 12], b"video\0"].concat(),
        );
        let vmhd = full(b"vmhd", 1, &[0; 8]);
        let dinf = mp4_box(
            b"dinf",
            &full(b"dref", 0, &[&be32(1)[..], &full(b"url ", 1, &[])].concat()),
        );
        let mut av01 = vec![0u8; 6];
        av01.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
        av01.extend_from_slice(&[0; 12]);
        av01.extend_from_slice(&w16.to_be_bytes());
        av01.extend_from_slice(&h16.to_be_bytes());
        av01.extend_from_slice(&[0, 0x48, 0, 0, 0, 0x48, 0, 0, 0, 0, 0, 0, 0, 1]);
        av01.extend_from_slice(&[0; 32]);
        av01.extend_from_slice(&[0, 0x18, 0xff, 0xff]);
        av01.extend_from_slice(&mp4_box(b"av1C", av1c));
        let stsd = full(
            b"stsd",
            0,
            &[&be32(1)[..], &mp4_box(b"av01", &av01)].concat(),
        );
        // (count, time) runs: a held step is one sample with its whole time
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for x in samples {
            match runs.last_mut() {
                Some((count, ms)) if *ms == x.ms => *count += 1,
                _ => runs.push((1, x.ms)),
            }
        }
        let mut times = be32(u32::try_from(runs.len()).unwrap_or(0)).to_vec();
        runs.iter()
            .for_each(|(c, ms)| times.extend([be32(*c), be32(*ms)].concat()));
        let stts = full(b"stts", 0, &times);
        let stsc = full(
            b"stsc",
            0,
            &[&be32(1)[..], &be32(1), &be32(n), &be32(1)].concat(),
        );
        let mut sizes = be32(0).to_vec();
        sizes.extend_from_slice(&be32(n));
        for x in samples {
            sizes.extend_from_slice(&be32(u32::try_from(x.data.len()).unwrap_or(u32::MAX)));
        }
        let stsz = full(b"stsz", 0, &sizes);
        let stco = full(b"stco", 0, &[&be32(1)[..], &be32(chunk_at)].concat());
        // the key frames, by sample number (from 1)
        let keys: Vec<u32> = (1..=n)
            .zip(samples)
            .filter(|(_, x)| x.key)
            .map(|(i, _)| i)
            .collect();
        let mut stss = be32(u32::try_from(keys.len()).unwrap_or(0)).to_vec();
        keys.iter().for_each(|k| stss.extend_from_slice(&be32(*k)));
        let stss = full(b"stss", 0, &stss);
        let stbl = mp4_box(b"stbl", &[stsd, stts, stss, stsc, stsz, stco].concat());
        let minf = mp4_box(b"minf", &[vmhd, dinf, stbl].concat());
        let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
        let trak = mp4_box(b"trak", &[tkhd, mdia].concat());
        mp4_box(b"moov", &[mvhd, trak].concat())
    };
    let at = u32::try_from(ftyp.len() + moov(0).len() + 8).map_err(|_| too_big())?;
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|x| x.data.iter().copied())
        .collect();
    Ok([ftyp, moov(at), mp4_box(b"mdat", &data)].concat())
}

/// An EBML element: id, size (shortest vint), body.
fn ebml(id: &[u8], body: &[u8]) -> Vec<u8> {
    let n = body.len() as u64;
    let len = (1..=8).find(|l| n < (1u64 << (7 * l)) - 1).unwrap_or(8);
    let mut out = id.to_vec();
    let size = n | (1u64 << (7 * len));
    out.extend_from_slice(&size.to_be_bytes()[8 - len as usize..]);
    out.extend_from_slice(body);
    out
}

/// A signed integer element (two's complement, the fewest bytes).
fn sint(id: &[u8], v: i64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let mut skip = 0;
    while skip < 7
        && ((bytes[skip] == 0 && bytes[skip + 1] < 0x80)
            || (bytes[skip] == 0xFF && bytes[skip + 1] >= 0x80))
    {
        skip += 1;
    }
    ebml(id, &bytes[skip..])
}

fn uint(id: &[u8], v: u64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count().min(7);
    ebml(id, &bytes[skip..])
}

/// The samples in a WebM: the EBML header, then one segment: info, the track
/// (`V_AV1`, the `av1C` as its private data) and a cluster a frame (so no
/// block's time needs more than the cluster's own).
pub fn webm(w: u32, h: u32, av1c: &[u8], samples: &[Sample]) -> Result<Vec<u8>, String> {
    let header = ebml(
        &[0x1A, 0x45, 0xDF, 0xA3],
        &[
            uint(&[0x42, 0x86], 1),
            uint(&[0x42, 0xF7], 1),
            uint(&[0x42, 0xF2], 4),
            uint(&[0x42, 0xF3], 8),
            ebml(&[0x42, 0x82], b"webm"),
            uint(&[0x42, 0x87], 4),
            uint(&[0x42, 0x85], 2),
        ]
        .concat(),
    );
    let total = samples.iter().map(|x| u64::from(x.ms)).sum::<u64>() as f64; // exact in an f64
    let info = ebml(
        &[0x15, 0x49, 0xA9, 0x66],
        &[
            uint(&[0x2A, 0xD7, 0xB1], 1_000_000),
            ebml(&[0x44, 0x89], &total.to_be_bytes()),
            ebml(&[0x4D, 0x80], b"OpenCircle"),
            ebml(&[0x57, 0x41], b"OpenCircle"),
        ]
        .concat(),
    );
    let video = ebml(
        &[0xE0],
        &[uint(&[0xB0], u64::from(w)), uint(&[0xBA], u64::from(h))].concat(),
    );
    let track = ebml(
        &[0xAE],
        &[
            uint(&[0xD7], 1),
            uint(&[0x73, 0xC5], 1),
            uint(&[0x83], 1),
            ebml(&[0x86], b"V_AV1"),
            ebml(&[0x63, 0xA2], av1c),
            video,
        ]
        .concat(),
    );
    let tracks = ebml(&[0x16, 0x54, 0xAE, 0x6B], &track);
    // a cluster a frame (so no block's time needs more than the cluster's own)
    let (mut clusters, mut cues) = (Vec::new(), Vec::new());
    let before = (info.len() + tracks.len()) as u64; // the segment's data starts at info
    let mut time = 0u64;
    for (i, x) in samples.iter().enumerate() {
        let ms = u64::from(x.ms);
        let at_time = time;
        time += ms;
        let time = at_time;
        // track 1, time 0 in the cluster; the last frame carries its duration
        // (a block group: a key frame has no reference, another one the frame before)
        let block = |flags: u8| [&[0x81, 0, 0, flags][..], &x.data].concat();
        let body = match i + 1 == samples.len() {
            false => ebml(&[0xA3], &block(if x.key { 0x80 } else { 0 })),
            true => {
                let mut group = ebml(&[0xA1], &block(0));
                group.extend(uint(&[0x9B], ms));
                if !x.key {
                    group.extend(sint(
                        &[0xFB],
                        -(i.checked_sub(1).map_or(ms, |p| u64::from(samples[p].ms)) as i64),
                    ));
                }
                ebml(&[0xA0], &group)
            }
        };
        if x.key {
            let at = before + clusters.len() as u64;
            let place = ebml(&[0xB7], &[uint(&[0xF7], 1), uint(&[0xF1], at)].concat());
            cues.extend(ebml(&[0xBB], &[uint(&[0xB3], time), place].concat()));
        }
        clusters.extend(ebml(
            &[0x1F, 0x43, 0xB6, 0x75],
            &[uint(&[0xE7], time), body].concat(),
        ));
    }
    let cues = ebml(&[0x1C, 0x53, 0xBB, 0x6B], &cues);
    let segment = ebml(
        &[0x18, 0x53, 0x80, 0x67],
        &[info, tracks, clusters, cues].concat(),
    );
    Ok([header, segment].concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::settings::Av1Decoder as Decoder;
    use crate::features::snip::export::{Movie, Walkthrough, encode_walkthrough};

    const COLOURS: [[u8; 3]; 3] = [[230, 30, 30], [30, 200, 40], [30, 40, 230]];

    fn frames() -> Vec<Crop> {
        COLOURS
            .iter()
            .map(|c| Crop {
                width: 64,
                height: 48,
                rgba: (0..64 * 48).flat_map(|_| [c[0], c[1], c[2], 255]).collect(),
            })
            .collect()
    }

    fn make(movie: Movie) -> Vec<u8> {
        let how = Walkthrough {
            movie,
            max_edge: 0,
            frame_ms: 500,
            transition: crate::features::snip::transition::Transition::Cut,
            ..Walkthrough::default()
        };
        let mut seen = Vec::new();
        let bytes = encode_walkthrough(frames(), how, &mut |d, t| {
            seen.push((d, t));
            true
        })
        .unwrap()
        .unwrap();
        assert_eq!(seen, [(1, 3), (2, 3), (3, 3)], "progress after every frame");
        bytes
    }

    /// The first child box `name` inside `data` (a box list).
    fn child<'a>(data: &'a [u8], name: &[u8; 4]) -> Option<&'a [u8]> {
        let mut at = 0;
        while at + 8 <= data.len() {
            let size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            if &data[at + 4..at + 8] == name {
                return Some(&data[at + 8..at + size]);
            }
            at += size;
        }
        None
    }

    fn be(d: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(d[at..at + 4].try_into().unwrap())
    }

    /// Decodes each sample: the size and the dominant colour channel.
    fn check_samples(samples: &[&[u8]]) {
        let mut decoder = Decoder::new(1).unwrap();
        for (i, s) in samples.iter().enumerate() {
            let mut seen = None;
            decoder
                .decode(s, |p| seen = Some((p.size(), p.rgba(64, 48).unwrap())))
                .unwrap();
            let (size, rgba) = seen.expect("a picture");
            assert_eq!(size, (64, 48));
            let c = &rgba[(24 * 64 + 32) * 4..][..3];
            let want = COLOURS[i];
            let big = (0..3).max_by_key(|&k| want[k]).unwrap();
            assert!(c[big] > 150, "frame {i}: {c:?}");
        }
    }

    #[test]
    fn an_mp4_has_every_frame_its_time_its_size_and_decodable_samples() {
        let f = make(Movie::Mp4);
        assert_eq!(&f[4..8], b"ftyp");
        let moov = child(&f, b"moov").unwrap();
        let trak = child(moov, b"trak").unwrap();
        let tkhd = child(trak, b"tkhd").unwrap();
        assert_eq!((be(tkhd, 76) >> 16, be(tkhd, 80) >> 16), (64, 48));
        let mdia = child(trak, b"mdia").unwrap();
        let stbl = child(child(mdia, b"minf").unwrap(), b"stbl").unwrap();
        let stsd = child(stbl, b"stsd").unwrap();
        assert!(child(&stsd[8..], b"av01").is_some());
        let stts = child(stbl, b"stts").unwrap();
        assert_eq!((be(stts, 4), be(stts, 8), be(stts, 12)), (1, 3, 500));
        let stsz = child(stbl, b"stsz").unwrap();
        assert_eq!(be(stsz, 8), 3, "three samples");
        let mut at = be(child(stbl, b"stco").unwrap(), 8) as usize;
        let mdat = child(&f, b"mdat").unwrap();
        assert_eq!(at, f.len() - mdat.len(), "the chunk starts at mdat's data");
        let mut samples = Vec::new();
        for i in 0..3 {
            let n = be(stsz, 12 + 4 * i) as usize;
            samples.push(&f[at..at + n]);
            at += n;
        }
        assert_eq!(at, f.len(), "the samples fill mdat");
        check_samples(&samples);
    }

    /// An EBML element at `at`: (id, body start, body end).
    fn element(d: &[u8], at: usize) -> (u32, usize, usize) {
        let idlen = d[at].leading_zeros() as usize + 1;
        let id = d[at..at + idlen]
            .iter()
            .fold(0, |a, &b| a << 8 | u32::from(b));
        let sl = d[at + idlen].leading_zeros() as usize + 1;
        let mut size = u64::from(d[at + idlen] & (0xFFu8 >> sl));
        for &b in &d[at + idlen + 1..at + idlen + sl] {
            size = size << 8 | u64::from(b);
        }
        let start = at + idlen + sl;
        (id, start, start + size as usize)
    }

    fn elements(d: &[u8]) -> Vec<(u32, &[u8])> {
        let mut at = 0;
        let mut out = Vec::new();
        while at < d.len() {
            let (id, s, e) = element(d, at);
            out.push((id, &d[s..e]));
            at = e;
        }
        out
    }

    fn find<'a>(list: &[(u32, &'a [u8])], id: u32) -> Option<&'a [u8]> {
        list.iter().find(|(i, _)| *i == id).map(|(_, b)| *b)
    }

    fn uint_of(b: &[u8]) -> u64 {
        b.iter().fold(0, |a, &x| a << 8 | u64::from(x))
    }

    /// Each cluster's time, its block (track, time and flags, then data), and
    /// whether it is a key frame.
    fn webm_blocks<'a>(segment: &[(u32, &'a [u8])]) -> (Vec<u64>, Vec<&'a [u8]>, Vec<bool>) {
        let (mut times, mut blocks, mut keys) = (Vec::new(), Vec::new(), Vec::new());
        for (id, body) in segment.iter().filter(|(i, _)| *i == 0x1F43B675) {
            let _ = id;
            let c = elements(body);
            times.push(uint_of(find(&c, 0xE7).unwrap()));
            match find(&c, 0xA3) {
                Some(b) => {
                    keys.push(b[3] & 0x80 != 0);
                    blocks.push(b);
                }
                None => {
                    let g = elements(find(&c, 0xA0).unwrap());
                    blocks.push(find(&g, 0xA1).unwrap());
                    keys.push(find(&g, 0xFB).is_none());
                }
            }
        }
        (times, blocks, keys)
    }

    /// Where each cluster starts, from the segment's data start.
    fn cluster_offsets(segment: &[(u32, &[u8])]) -> Vec<u64> {
        let mut at = 0u64;
        let mut out = Vec::new();
        for (id, body) in segment {
            let head = match id {
                0x1F43B675 | 0x1549A966 | 0x1654AE6B | 0x1C53BB6B => 4,
                _ => 0,
            };
            if *id == 0x1F43B675 {
                out.push(at);
            }
            // id bytes + the size's bytes + the body
            let size_len = (1..=8)
                .find(|l| (body.len() as u64) < (1u64 << (7 * l)) - 1)
                .unwrap();
            at += (head + size_len + body.len()) as u64;
        }
        out
    }

    /// A cue point's cluster position.
    fn cue_position(cue: &(u32, &[u8])) -> u64 {
        let point = elements(cue.1);
        let place = elements(find(&point, 0xB7).unwrap());
        uint_of(find(&place, 0xF1).unwrap())
    }

    #[test]
    fn key_frames_are_marked_in_both_containers_and_the_webm_cues_list_them() {
        let five: Vec<Crop> = frames().into_iter().cycle().take(5).collect();
        let (av1c, samples) = encode_av1(
            five.iter().map(|f| (std::borrow::Cow::Borrowed(f), 500)),
            5,
            2,
            &mut |_, _| true,
        )
        .unwrap()
        .unwrap();
        let keys: Vec<bool> = samples.iter().map(|s| s.key).collect();
        assert_eq!(
            keys,
            [true, false, true, false, true],
            "a key frame every 2"
        );
        // MP4: the sync sample table lists samples 1, 3, 5
        let f = mp4(64, 48, &av1c, &samples).unwrap();
        let stbl = child(
            child(
                child(child(&f[..], b"moov").unwrap(), b"trak").unwrap(),
                b"mdia",
            )
            .unwrap(),
            b"minf",
        )
        .and_then(|m| child(m, b"stbl"))
        .unwrap();
        let stss = child(stbl, b"stss").unwrap();
        assert_eq!(be(stss, 4), 3);
        assert_eq!((be(stss, 8), be(stss, 12), be(stss, 16)), (1, 3, 5));
        // WebM: the key flag on the blocks, and one cue a key frame at the right place
        let f = webm(64, 48, &av1c, &samples).unwrap();
        let top = elements(&f);
        let segment = elements(find(&top, 0x18538067).unwrap());
        let (times, _, flags) = webm_blocks(&segment);
        assert_eq!(flags, keys);
        let cues = elements(find(&segment, 0x1C53BB6B).unwrap());
        assert_eq!(cues.len(), 3);
        let offsets = cluster_offsets(&segment);
        for (cue, k) in cues.iter().zip([0usize, 2, 4]) {
            assert_eq!(cue_position(cue), offsets[k]);
            assert_eq!(uint_of(find(&elements(cue.1), 0xB3).unwrap()), times[k]);
        }
    }

    #[test]
    fn a_video_too_big_for_its_container_is_an_error_not_a_wrap() {
        let s = [Sample {
            data: vec![1],
            key: true,
            ms: 500,
        }];
        assert!(mp4(70_000, 48, &[], &s).is_err());
        assert!(
            mp4(
                64,
                48,
                &[],
                &[s[0].data.clone(), s[0].data.clone()].map(|data| Sample {
                    data,
                    key: true,
                    ms: u32::MAX
                })
            )
            .is_err()
        );
    }

    #[test]
    fn a_webm_has_every_frame_its_time_its_size_and_decodable_samples() {
        let f = make(Movie::Webm);
        let top = elements(&f);
        let header = elements(find(&top, 0x1A45DFA3).unwrap());
        assert_eq!(find(&header, 0x4282).unwrap(), b"webm");
        let segment = elements(find(&top, 0x18538067).unwrap());
        let info = elements(find(&segment, 0x1549A966).unwrap());
        assert_eq!(
            f64::from_be_bytes(find(&info, 0x4489).unwrap().try_into().unwrap()),
            1500.0
        );
        let track = elements(find(&elements(find(&segment, 0x1654AE6B).unwrap()), 0xAE).unwrap());
        assert_eq!(find(&track, 0x86).unwrap(), b"V_AV1");
        let video = elements(find(&track, 0xE0).unwrap());
        assert_eq!(
            (
                uint_of(find(&video, 0xB0).unwrap()),
                uint_of(find(&video, 0xBA).unwrap())
            ),
            (64, 48)
        );
        let (times, samples, keys) = webm_blocks(&segment);
        assert_eq!(times, [0, 500, 1000]);
        assert_eq!(keys, [true, false, false]);
        let samples: Vec<&[u8]> = samples.iter().map(|b| &b[4..]).collect();
        check_samples(&samples);
        // the last frame is a block group with its duration (and, not a key, its reference)
        let last = elements(
            segment
                .iter()
                .rev()
                .find(|(i, _)| *i == 0x1F43B675)
                .unwrap()
                .1,
        );
        let group = elements(find(&last, 0xA0).unwrap());
        assert_eq!(uint_of(find(&group, 0x9B).unwrap()), 500);
        assert!(find(&group, 0xFB).is_some());
        // one cue, for the one key frame, pointing at its cluster
        let cues = elements(find(&segment, 0x1C53BB6B).unwrap());
        assert_eq!(cues.len(), 1);
        assert_eq!(cluster_offsets(&segment)[0], cue_position(&cues[0]));
    }

    #[test]
    fn a_cancel_stops_the_encoder() {
        let how = Walkthrough {
            movie: Movie::Mp4,
            max_edge: 0,
            transition: crate::features::snip::transition::Transition::Cut,
            ..Walkthrough::default()
        };
        let out = encode_walkthrough(frames(), how, &mut |d, _| d < 1).unwrap();
        assert!(out.is_none());
    }

    /// The lab's numbers (`cargo test -- --ignored --nocapture lab_`): 6 frames
    /// of a screenshot-like picture, each format, time and size.
    #[test]
    #[ignore]
    fn lab_timing() {
        let (w, h) = (1280u32, 720u32);
        let frames: Vec<Crop> = (0..6u32)
            .map(|n| Crop {
                width: w,
                height: h,
                rgba: (0..w * h)
                    .flat_map(|i| {
                        let (x, y) = (i % w, i / w);
                        match (
                            y < 48,
                            x < 220,
                            y / 16 % 2 == 0 && x > 260 && x < 260 + (y * 7 + n * 131) % 700,
                        ) {
                            (true, _, _) => [40, 44, 52, 255],
                            (_, true, _) => [230, 232, 236, 255],
                            (_, _, true) => [60, 60, 70, 255],
                            _ => [245, 245, 247, 255],
                        }
                    })
                    .collect(),
            })
            .collect();
        for movie in Movie::ALL {
            let how = Walkthrough {
                movie,
                max_edge: 0,
                transition: crate::features::snip::transition::Transition::Cut,
                ..Walkthrough::default()
            };
            let t = std::time::Instant::now();
            let out = encode_walkthrough(frames.clone(), how, &mut |_, _| true)
                .unwrap()
                .unwrap();
            println!("{}: {} bytes, {:?}", movie.name(), out.len(), t.elapsed());
        }
    }

    /// W91.3: with a transition a held step is ONE sample with its whole time and only
    /// the mixed frames are 50 ms samples: 3 steps of 1 s, 0.3 s between = 15 samples,
    /// 3.6 s, in both containers.
    #[test]
    fn a_held_step_is_one_sample_with_its_whole_time() {
        use crate::features::snip::transition::Transition;
        let make = |movie| {
            let how = Walkthrough {
                movie,
                max_edge: 0,
                frame_ms: 1000,
                transition: Transition::Crossfade,
                transition_ms: 300,
                ..Walkthrough::default()
            };
            encode_walkthrough(frames(), how, &mut |_, _| true)
                .unwrap()
                .unwrap()
        };
        let f = make(Movie::Mp4);
        let mdia = child(child(&f, b"moov").unwrap(), b"trak")
            .and_then(|t| child(t, b"mdia"))
            .unwrap();
        let stbl = child(child(mdia, b"minf").unwrap(), b"stbl").unwrap();
        let stts = child(stbl, b"stts").unwrap();
        assert_eq!(be(stts, 4), 5, "hold, mixes, hold, mixes, hold");
        let runs: Vec<(u32, u32)> = (0..5)
            .map(|k| (be(stts, 8 + 8 * k), be(stts, 12 + 8 * k)))
            .collect();
        assert_eq!(runs, [(1, 1000), (6, 50), (1, 1000), (6, 50), (1, 1000)]);
        assert_eq!(be(child(stbl, b"stsz").unwrap(), 8), 15);
        assert_eq!(be(child(mdia, b"mdhd").unwrap(), 16), 3600, "total ms");
        let f = make(Movie::Webm);
        let top = elements(&f);
        let segment = elements(find(&top, 0x18538067).unwrap());
        let info = elements(find(&segment, 0x1549A966).unwrap());
        assert_eq!(
            f64::from_be_bytes(find(&info, 0x4489).unwrap().try_into().unwrap()),
            3600.0
        );
        let (times, _, _) = webm_blocks(&segment);
        assert_eq!(times.len(), 15);
        assert_eq!(
            (times[0], times[1], times[7], times[8]),
            (0, 1000, 1300, 2300)
        );
        assert_eq!(
            times[14], 2600,
            "the last step starts after two holds and two transitions"
        );
    }
}
