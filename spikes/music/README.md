# Spike: song search (AcoustID) — verdict: not viable

Throwaway. Measured 2026-10-24.

`music-spike <raw s16le mono 11025 file> [start_s] [len_s]` fingerprints a file or clip;
`music-spike rec <secs>` records what the PC plays (Linux: `@DEFAULT_MONITOR@` over the
PulseAudio protocol with the pure-Rust `pulseaudio` crate, works on PipeWire's pulse
server; Windows: an input stream on the default output device = WASAPI loopback, `cpal`).
Both print `duration fingerprint ms`. Lookups: AcoustID `/v2/lookup` with its documented
test key (expires after a few days; a real app key needs a signed-in registration).

## Results (14 CC-BY Kevin MacLeod tracks, all in MusicBrainz)

| Input | Hits |
|---|---|
| Whole track, real duration | 14/14 (score 1.0; 12 with title/artist, 2 id only) |
| 10 s clip (at 0, 30 s), clip duration | 0/28 |
| 10 s clip, real track duration | 1/28 (90 s in, score 0.89) |
| 20 s / 30 s clip from the start | 0/14, 1/14 id-only |
| 30 s clip at 30 s, real duration | 3/14, one of them wrong (audiobook that uses the track) |
| 60 s from the start | 1/14 (the 67 s track, i.e. nearly whole) |
| 60 s at 30 s, real duration | 3/14, one wrong |
| Ubuntu VM: 10 s captured from the monitor while playing | 0/1 |
| Ubuntu VM: whole 67 s track captured from the monitor | 1/1 (Fluffing a Duck) |

Fingerprint: 10 s clip < 2 ms, 3.5 min track ~25 ms (release). Lookup: 300–480 ms
(one 1.3 s outlier). Size: chromaprint + rustfft + rubato + pulseaudio ≈ +1.1 MB (Linux,
stripped); chromaprint + cpal ≈ +1.1 MB (Windows). Windows VM was unreachable, so
WASAPI loopback was built and linted but not run.

## Why

AcoustID matches whole files: the server filters candidates by the track's duration
and aligns against the stored full-track fingerprint. A snippet heard mid-song has
neither. It is a tagger for files, not a Shazam-style snippet recogniser, and no
client-side trick fixes that (we can't know the song's length; guessing it means
dozens of requests per press against a 3 req/s limit, and still wrong matches).

Snippet recognition needs a landmark/peak-hash index (Shazam, ACRCloud, AudD). None
is free + documented + private: Shazam has no public API (clients use a
reverse-engineered endpoint), ACRCloud and AudD are paid with small trial tiers.
