//! The walkthrough as a video (v0.4.3): AV1 by rav1e (BSD-2, pure Rust, no
//! asm) in an MP4 or a WebM file, each written by the small muxer here
//! (one video track, one sample a frame; no crate: the two layouts are a page
//! each). Frames come in already the same size, even width and height.
use rav1e::prelude::*;

use crate::shared::picture::Crop;

/// A frame's time and the encoder's choices. Quantizer 100 of 255: text on a
/// screenshot stays sharp, a flat screen costs little.
const QUANTIZER: usize = 100;

/// The `av1C` record and one sample a frame.
pub type Encoded = (Vec<u8>, Vec<Vec<u8>>);

/// `frames` (same size, even sides) as AV1: the `av1C` record and one sample
/// each. `progress(done, total)` after every sample: false = cancel (`None`).
pub fn encode_av1(
    frames: &[Crop],
    frame_ms: u32,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<Encoded>, String> {
    let first = frames.first().ok_or("no frames")?;
    let (w, h) = (first.width as usize, first.height as usize);
    let mut enc = EncoderConfig::with_speed_preset(10);
    enc.width = w;
    enc.height = h;
    enc.time_base = Rational::new(u64::from(frame_ms), 1000);
    enc.low_latency = true;
    enc.quantizer = QUANTIZER;
    enc.color_description = Some(ColorDescription {
        color_primaries: ColorPrimaries::BT709,
        transfer_characteristics: TransferCharacteristics::BT709,
        matrix_coefficients: MatrixCoefficients::BT709,
    });
    let cfg = Config::new().with_encoder_config(enc).with_threads(0);
    let mut ctx: Context<u8> = cfg.new_context().map_err(|e| e.to_string())?;
    let mut samples = Vec::new();
    let total = frames.len();
    let mut take = |ctx: &mut Context<u8>, samples: &mut Vec<Vec<u8>>| -> Result<bool, String> {
        loop {
            match ctx.receive_packet() {
                Ok(p) => {
                    samples.push(strip_delimiter(p.data));
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
    for f in frames {
        let mut frame = ctx.new_frame();
        let (y, u, v) = yuv420(f);
        frame.planes[0].copy_from_raw_u8(&y, w, 1);
        frame.planes[1].copy_from_raw_u8(&u, w / 2, 1);
        frame.planes[2].copy_from_raw_u8(&v, w / 2, 1);
        ctx.send_frame(frame).map_err(|e| e.to_string())?;
        if !take(&mut ctx, &mut samples)? {
            return Ok(None);
        }
    }
    ctx.flush();
    if !take(&mut ctx, &mut samples)? {
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
pub fn mp4(w: u32, h: u32, frame_ms: u32, av1c: &[u8], samples: &[Vec<u8>]) -> Vec<u8> {
    let ftyp = mp4_box(
        b"ftyp",
        &[b"isom".as_slice(), &be32(0), b"isomiso2av01mp41"].concat(),
    );
    let n = samples.len() as u32;
    let total = n * frame_ms;
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
                &be32(w << 16),
                &be32(h << 16),
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
        av01.extend_from_slice(&(w as u16).to_be_bytes());
        av01.extend_from_slice(&(h as u16).to_be_bytes());
        av01.extend_from_slice(&[0, 0x48, 0, 0, 0, 0x48, 0, 0, 0, 0, 0, 0, 0, 1]);
        av01.extend_from_slice(&[0; 32]);
        av01.extend_from_slice(&[0, 0x18, 0xff, 0xff]);
        av01.extend_from_slice(&mp4_box(b"av1C", av1c));
        let stsd = full(
            b"stsd",
            0,
            &[&be32(1)[..], &mp4_box(b"av01", &av01)].concat(),
        );
        let stts = full(
            b"stts",
            0,
            &[&be32(1)[..], &be32(n), &be32(frame_ms)].concat(),
        );
        let stsc = full(
            b"stsc",
            0,
            &[&be32(1)[..], &be32(1), &be32(n), &be32(1)].concat(),
        );
        let mut sizes = be32(0).to_vec();
        sizes.extend_from_slice(&be32(n));
        samples
            .iter()
            .for_each(|s| sizes.extend_from_slice(&be32(s.len() as u32)));
        let stsz = full(b"stsz", 0, &sizes);
        let stco = full(b"stco", 0, &[&be32(1)[..], &be32(chunk_at)].concat());
        let stss = full(b"stss", 0, &[&be32(1)[..], &be32(1)].concat());
        let stbl = mp4_box(b"stbl", &[stsd, stts, stss, stsc, stsz, stco].concat());
        let minf = mp4_box(b"minf", &[vmhd, dinf, stbl].concat());
        let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
        let trak = mp4_box(b"trak", &[tkhd, mdia].concat());
        mp4_box(b"moov", &[mvhd, trak].concat())
    };
    let at = (ftyp.len() + moov(0).len() + 8) as u32;
    let data: Vec<u8> = samples.concat();
    [ftyp, moov(at), mp4_box(b"mdat", &data)].concat()
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

fn uint(id: &[u8], v: u64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count().min(7);
    ebml(id, &bytes[skip..])
}

/// The samples in a WebM: the EBML header, then one segment: info, the track
/// (`V_AV1`, the `av1C` as its private data) and a cluster a frame (so no
/// block's time needs more than the cluster's own).
pub fn webm(w: u32, h: u32, frame_ms: u32, av1c: &[u8], samples: &[Vec<u8>]) -> Vec<u8> {
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
    let total = f64::from(frame_ms) * samples.len() as f64;
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
    let clusters: Vec<u8> = (samples.iter().enumerate())
        .flat_map(|(i, s)| {
            // track 1, time 0 in the cluster, flags: the first frame is a key frame
            let block = [&[0x81, 0, 0, if i == 0 { 0x80 } else { 0 }][..], s].concat();
            ebml(
                &[0x1F, 0x43, 0xB6, 0x75],
                &[
                    uint(&[0xE7], i as u64 * u64::from(frame_ms)),
                    ebml(&[0xA3], &block),
                ]
                .concat(),
            )
        })
        .collect();
    let segment = ebml(
        &[0x18, 0x53, 0x80, 0x67],
        &[info, tracks, clusters].concat(),
    );
    [header, segment].concat()
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
        let (mut times, mut samples) = (Vec::new(), Vec::new());
        for (id, body) in &segment {
            if *id == 0x1F43B675 {
                let c = elements(body);
                times.push(uint_of(find(&c, 0xE7).unwrap()));
                samples.push(&find(&c, 0xA3).unwrap()[4..]);
            }
        }
        assert_eq!(times, [0, 500, 1000]);
        check_samples(&samples);
    }

    #[test]
    fn a_cancel_stops_the_encoder() {
        let how = Walkthrough {
            movie: Movie::Mp4,
            max_edge: 0,
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
                ..Walkthrough::default()
            };
            let t = std::time::Instant::now();
            let out = encode_walkthrough(frames.clone(), how, &mut |_, _| true)
                .unwrap()
                .unwrap();
            println!("{}: {} bytes, {:?}", movie.name(), out.len(), t.elapsed());
        }
    }
}
