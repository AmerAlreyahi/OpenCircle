# Song search

Name the song that's playing: in a video, a game, a stream, or (with the
microphone) in the room. Song search is **off until you add your own key** for
a recognition service. Without one, the music button doesn't appear.

## Why a key

Free song databases (AcoustID / MusicBrainz) match whole audio files, not a
few seconds heard in the middle of a song: in our tests a 10-second clip found
1 song in 28. Recognising a short clip needs a service like AudD, which wants
an account.

## Set it up

1. Sign up at [dashboard.audd.io](https://dashboard.audd.io) and copy your API
   token. AudD's plans and free allowance are on its site.
2. Open **Settings → Song search**, paste the token, press **Save key**. It goes
   to your system keyring (Windows Credential Manager, or GNOME Keyring /
   KWallet on Linux), never into a file.
3. Choose what to listen to: **This PC** (what your speakers or headphones
   play) or **Microphone**.

## Use it

Open OpenCircle (`Ctrl+Shift+Space`) and press the music note at the bottom.
OpenCircle listens for 10 seconds ("Listening… 8 s", with Cancel), then shows
the song: cover, title, artist, album and year, with Spotify, YouTube and More
links. A link opens in your browser only when you click it.

You can also give it its own shortcut (**Settings → Shortcuts → Song search**,
unbound by default) or run it from a terminal:

```bash
opencircle run song_search
```

If it says "No sound is playing", start the song (and check its volume isn't
muted); "No match" usually means too much talking or noise over the music.

## History

Off by default. Turn it on in **Settings → Song search** to see your recent
songs under each result. They are kept only on this PC, in
`song_history.json` in your local data folder; turning history off deletes
them at the next search.

## Privacy

When you press the music button, a ~10-second clip of the audio you choose is
sent to AudD. Nothing is recorded at any other time, and the clip is never
saved. See [privacy](privacy.md).

## Platforms

- **Windows**: "This PC" uses WASAPI loopback of the default output device.
- **Linux**: "This PC" records the default output's monitor through the
  PulseAudio protocol, which PipeWire also provides (`pipewire-pulse`, the
  default on current Ubuntu and Fedora).
