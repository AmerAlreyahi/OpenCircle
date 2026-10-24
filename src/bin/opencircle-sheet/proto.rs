//! What the app (`src/sheet.rs`) and the results-card helper (`opencircle-sheet`)
//! share: the helper's command line, the lines it prints, the card's geometry
//! and the page filter. Plain std only: the helper includes this file, so do
//! the app's tests.
//!
//! App → helper: `opencircle-sheet <lens|ai> [--near X] [--size WxH]
//! --colours BG,BORDER,HOVER,TEXT,MUTED --shadow A`, then on stdin the page
//! (Lens: the auto-submit HTML, from memory, never a file) or the AI Mode URL.
//! Helper → app, one per line on stdout: `ready` (shown), `open <url>` (to the
//! user's browser), `size W H` / `size default` (what the user resized it to).

/// The card's geometry: the same numbers as `ui::widgets::card` (a test in
/// `src/sheet.rs` keeps them equal).
pub const GAP: f64 = 20.0;
pub const SHARE: f64 = 0.4;
pub const WIDTH: f64 = 420.0;
pub const MIN: (f64, f64) = (320.0, 220.0);
pub const NARROW: f64 = 900.0;
pub const BOTTOM_WIDTH: f64 = 640.0;
pub const BOTTOM_MIN: f64 = 260.0;

/// x, y, w, h (logical px).
pub type Rect4 = [f64; 4];

/// The one message the page may send the helper (Esc inside the page).
pub const CLOSE: &str = "close";
/// Where AI Mode questions go (the app builds the URL; the helper checks it).
pub const AI_PREFIX: &str = "https://www.google.com/search?";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Lens,
    Ai,
}

impl Kind {
    pub fn arg(self) -> &'static str {
        match self {
            Kind::Lens => "lens",
            Kind::Ai => "ai",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Lens => "Google · Lens",
            Kind::Ai => "Google · AI Mode",
        }
    }

    /// The footer's privacy line.
    pub fn privacy(self) -> &'static str {
        match self {
            Kind::Lens => "Only this selection is sent, and only to Google.",
            Kind::Ai => "Only your question and the text read from the selection go to Google.",
        }
    }
}

/// The helper's command line.
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub kind: Kind,
    /// The selection's centre, as a share (0..1) of the screen width.
    pub near: Option<f64>,
    /// The size the user last resized the card to (this session).
    pub size: Option<(f64, f64)>,
    /// 0xRRGGBB: background, border, hover (chip, buttons), text, muted.
    pub colours: [u32; 5],
    /// The shadow's opacity.
    pub shadow: f64,
}

impl Args {
    pub fn to_vec(&self) -> Vec<String> {
        let mut v = vec![self.kind.arg().to_string()];
        if let Some(x) = self.near {
            v.extend(["--near".into(), format!("{x:.4}")]);
        }
        if let Some((w, h)) = self.size {
            v.extend(["--size".into(), format!("{}x{}", w.round(), h.round())]);
        }
        let colours: Vec<_> = self.colours.iter().map(|c| format!("{c:06x}")).collect();
        v.extend(["--colours".into(), colours.join(",")]);
        v.extend(["--shadow".into(), format!("{:.2}", self.shadow)]);
        v
    }

    pub fn parse(args: &[String]) -> Result<Args, String> {
        let kind = match args.first().map(String::as_str) {
            Some("lens") => Kind::Lens,
            Some("ai") => Kind::Ai,
            _ => return Err("expected lens or ai".into()),
        };
        let mut a = Args {
            kind,
            near: None,
            size: None,
            colours: [0x1c1c1e, 0x3a3a3c, 0x2c2c2e, 0xffffff, 0x8e8e93],
            shadow: 0.35,
        };
        let mut it = args[1..].iter();
        while let Some(flag) = it.next() {
            let value = it.next().ok_or(format!("{flag}: no value"))?;
            let bad = || format!("{flag}: bad value {value:?}");
            match flag.as_str() {
                "--near" => {
                    let x: f64 = value.parse().map_err(|_| bad())?;
                    a.near = Some(x).filter(|x| (0.0..=1.0).contains(x));
                }
                "--size" => {
                    let (w, h) = value.split_once('x').ok_or_else(bad)?;
                    let (w, h): (f64, f64) =
                        (w.parse().map_err(|_| bad())?, h.parse().map_err(|_| bad())?);
                    a.size = Some((w, h)).filter(|&(w, h)| w > 0.0 && h > 0.0 && w.is_finite());
                }
                "--colours" => {
                    let c: Vec<u32> = value
                        .split(',')
                        .map(|c| u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6))
                        .collect::<Option<_>>()
                        .ok_or_else(bad)?;
                    a.colours = c.try_into().map_err(|_| bad())?;
                }
                "--shadow" => a.shadow = value.parse::<f64>().map_err(|_| bad())?.clamp(0.0, 1.0),
                _ => return Err(format!("unknown option {flag}")),
            }
        }
        Ok(a)
    }
}

/// The side the card hugs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Left,
    Right,
    Bottom,
}

/// The card in an `area` (w, h): `ui::widgets::card::place`'s rule, but only
/// the size is remembered, never the position. On the side away from the
/// selection (`near`: its centre, 0..1 of the width; none = the right), full
/// height, `SHARE` of the width (at least `WIDTH`); narrow or portrait: at
/// the bottom, centred. `size` (the user's last resize) sets the width of a
/// side card, the height of a bottom one.
pub fn place(area: (f64, f64), near: Option<f64>, size: Option<(f64, f64)>) -> (Place, Rect4) {
    let (w, h) = area;
    let inner = ((w - 2.0 * GAP).max(0.0), (h - 2.0 * GAP).max(0.0));
    let fit = |v: f64, min: f64, max: f64| v.clamp(min.min(max), max);
    if w < NARROW || h > w {
        let width = inner.0.min(BOTTOM_WIDTH);
        let height = match size {
            Some((_, sh)) => fit(sh, MIN.1, inner.1),
            None => (h / 2.0).max(BOTTOM_MIN).min(inner.1),
        };
        let r = [(w - width) / 2.0, h - GAP - height, width, height];
        return (Place::Bottom, r);
    }
    let width = match size {
        Some((sw, _)) => fit(sw, MIN.0, inner.0),
        None => (w * SHARE).max(WIDTH).min(inner.0),
    };
    if near.is_some_and(|x| x >= 0.5) {
        (Place::Left, [GAP, GAP, width, inner.1])
    } else {
        (Place::Right, [w - GAP - width, GAP, width, inner.1])
    }
}

/// `r` resized by its inner edge by (dx, dy), at least `MIN` and `GAP` from
/// the area's edges (`ui::widgets::card::drag`'s resize).
pub fn resized(p: Place, r: Rect4, dx: f64, dy: f64, area: (f64, f64)) -> Rect4 {
    let [x0, y0, w, h] = r;
    let (x1, y1) = (x0 + w, y0 + h);
    let (x0, x1, y0) = match p {
        Place::Right => ((x0 + dx).clamp(GAP, (x1 - MIN.0).max(GAP)), x1, y0),
        Place::Left => (
            x0,
            (x1 + dx).clamp(x0 + MIN.0, (area.0 - GAP).max(x0 + MIN.0)),
            y0,
        ),
        Place::Bottom => (x0, x1, (y0 + dy).clamp(GAP, (y1 - MIN.1).max(GAP))),
    };
    [x0, y0, x1 - x0, y1 - y0]
}

// The card's frame, as `ui::widgets::card::frame` lays it out (logical px):
// header 44 + 2 x 8 padding, a 1 px rule, body padding 20, footer rule + 16 px
// padding around one 12 px line, 20 px corners, a 10 px resize strip on the
// inner edge between the corners, icon buttons 44 px, 12 px from the right.
pub const HEADER: f64 = 61.0;
pub const FOOTER: f64 = 49.0;
pub const BODY_PAD: f64 = 20.0;
pub const RADIUS: f64 = 20.0;
pub const EDGE: f64 = 10.0;
/// The page's own corners inside the body.
pub const BODY_R: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close,
    /// "Open in browser".
    Browser,
    /// The inner edge: drag to resize, double-click for the default size.
    Edge,
}

/// The × button's centre in a `w` wide card.
pub fn close_at(w: f64) -> (f64, f64) {
    (w - 12.0 - 22.0, 8.0 + 22.0)
}

/// The "Open in browser" button's centre: left of ×.
pub fn browser_at(w: f64) -> (f64, f64) {
    (w - 12.0 - 44.0 - 4.0 - 22.0, 8.0 + 22.0)
}

/// What a press at (x, y) in a `w` x `h` card on side `p` hits (None: the
/// page, or the plain frame: the card never moves, only its size changes).
pub fn hit(p: Place, x: f64, y: f64, w: f64, h: f64) -> Option<Hit> {
    let on = |(cx, cy): (f64, f64)| (x - cx).hypot(y - cy) < 16.0;
    let along_y = (RADIUS..h - RADIUS).contains(&y);
    let edge = match p {
        Place::Right => x < EDGE && along_y,
        Place::Left => x >= w - EDGE && along_y,
        Place::Bottom => y < EDGE && (RADIUS..w - RADIUS).contains(&x),
    };
    if on(close_at(w)) {
        Some(Hit::Close)
    } else if on(browser_at(w)) {
        Some(Hit::Browser)
    } else if edge {
        Some(Hit::Edge)
    } else {
        None
    }
}

/// The page's rect inside a `w` x `h` card.
pub fn body(w: f64, h: f64) -> Rect4 {
    [
        BODY_PAD,
        HEADER + BODY_PAD,
        (w - 2.0 * BODY_PAD).max(0.0),
        (h - HEADER - FOOTER - 2.0 * BODY_PAD).max(0.0),
    ]
}

/// What may load in the card: https, and our own in-memory page (`own`).
/// Never file://, data:, javascript: or plain http.
pub fn allowed(url: &str, own: &str) -> bool {
    url.starts_with("https://") || url == own || url == "about:blank"
}

/// `scheme://host/path` only: the query can hold the user's text, never log it.
pub fn short(url: &str) -> &str {
    &url[..url.find(['?', '#']).unwrap_or(url.len())]
}

/// A line the helper prints for the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Ready,
    /// Open this in the user's browser (the app checks it is a plain http(s) link).
    Open(String),
    /// The user resized the card (`None`: back to the default size).
    Size(Option<(f64, f64)>),
}

impl Line {
    /// Anything else (the web engine's own chatter) is not a line for the app.
    pub fn parse(line: &str) -> Option<Line> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "ready" {
            return Some(Line::Ready);
        }
        if let Some(url) = line.strip_prefix("open ") {
            return (!url.is_empty() && !url.contains(char::is_whitespace))
                .then(|| Line::Open(url.into()));
        }
        let size = line.strip_prefix("size ")?;
        if size == "default" {
            return Some(Line::Size(None));
        }
        let (w, h) = size.split_once(' ')?;
        let (w, h): (f64, f64) = (w.parse().ok()?, h.parse().ok()?);
        (w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()).then_some(Line::Size(Some((w, h))))
    }

    pub fn text(&self) -> String {
        match self {
            Line::Ready => "ready".into(),
            Line::Open(url) => format!("open {url}"),
            Line::Size(None) => "size default".into(),
            Line::Size(Some((w, h))) => format!("size {} {}", w.round(), h.round()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_round_trip_and_reject_junk() {
        let a = Args {
            kind: Kind::Ai,
            near: Some(0.25),
            size: Some((700.0, 900.0)),
            colours: [0x112233, 0x445566, 0x778899, 0xaabbcc, 0xddeeff],
            shadow: 0.5,
        };
        assert_eq!(Args::parse(&a.to_vec()), Ok(a));
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(Args::parse(&s(&["lens"])).unwrap().kind, Kind::Lens);
        for bad in [
            &["x"][..],
            &[],
            &["ai", "--near"],
            &["ai", "--size", "10"],
            &["ai", "--colours", "12345,1,2,3,4"],
            &["ai", "--colours", "112233"],
            &["ai", "--evil", "1"],
        ] {
            assert!(Args::parse(&s(bad)).is_err(), "{bad:?}");
        }
        // out of range: ignored, not trusted
        let a = Args::parse(&s(&["ai", "--near", "7", "--size", "-1x5"])).unwrap();
        assert_eq!((a.near, a.size), (None, None));
    }

    #[test]
    fn card_goes_away_from_the_selection_and_keeps_only_its_size() {
        let area = (1920.0, 1080.0);
        let right = [1132.0, 20.0, 768.0, 1040.0];
        assert_eq!(place(area, Some(0.2), None), (Place::Right, right));
        assert_eq!(place(area, None, None), (Place::Right, right));
        assert_eq!(
            place(area, Some(0.8), None),
            (Place::Left, [20.0, 20.0, 768.0, 1040.0])
        );
        // A remembered size: the width; the side still follows the selection.
        let mine = Some((500.0, 300.0));
        assert_eq!(
            place(area, Some(0.8), mine),
            (Place::Left, [20.0, 20.0, 500.0, 1040.0])
        );
        assert_eq!(
            place(area, Some(0.1), mine),
            (Place::Right, [1400.0, 20.0, 500.0, 1040.0])
        );
        // ... within bounds
        assert_eq!(place(area, None, Some((10.0, 1.0))).1[2], MIN.0);
        assert_eq!(place(area, None, Some((5000.0, 1.0))).1[2], 1880.0);
        // Portrait or narrow: bottom, centred; a remembered height.
        let (p, r) = place((1080.0, 1920.0), Some(0.1), None);
        assert_eq!((p, r), (Place::Bottom, [220.0, 940.0, 640.0, 960.0]));
        let (p, r) = place((800.0, 600.0), Some(0.9), Some((999.0, 400.0)));
        assert_eq!((p, r), (Place::Bottom, [80.0, 180.0, 640.0, 400.0]));
    }

    #[test]
    fn the_inner_edge_resizes_within_bounds() {
        let area = (1920.0, 1080.0);
        let (p, r) = place(area, None, None);
        assert_eq!(
            resized(p, r, -100.0, 0.0, area),
            [1032.0, 20.0, 868.0, 1040.0]
        );
        assert_eq!(resized(p, r, 900.0, 0.0, area)[2], MIN.0);
        assert_eq!(resized(p, r, -5000.0, 0.0, area)[0], GAP);
        let (p, r) = place(area, Some(0.9), None);
        assert_eq!(resized(p, r, 100.0, 0.0, area), [20.0, 20.0, 868.0, 1040.0]);
        assert_eq!(resized(p, r, 5000.0, 0.0, area)[2], 1880.0);
        let (p, r) = place((800.0, 600.0), None, None);
        assert_eq!(resized(p, r, 0.0, -50.0, (800.0, 600.0))[3], 350.0);
        assert_eq!(resized(p, r, 0.0, 500.0, (800.0, 600.0))[3], MIN.1);
    }

    #[test]
    fn buttons_and_the_inner_edge_only_the_header_does_not_move_it() {
        let (w, h) = (500.0, 800.0);
        let (cx, cy) = close_at(w);
        assert_eq!(hit(Place::Right, cx, cy, w, h), Some(Hit::Close));
        let (bx, by) = browser_at(w);
        assert_eq!(hit(Place::Right, bx, by, w, h), Some(Hit::Browser));
        assert_eq!(
            hit(Place::Right, 200.0, 20.0, w, h),
            None,
            "the header: no move"
        );
        assert_eq!(hit(Place::Right, 250.0, 400.0, w, h), None); // the page's
        assert_eq!(hit(Place::Right, 3.0, 400.0, w, h), Some(Hit::Edge));
        assert_eq!(hit(Place::Right, 3.0, 5.0, w, h), None); // a corner: not the edge
        assert_eq!(hit(Place::Right, 497.0, 400.0, w, h), None); // outer edge: not a handle
        assert_eq!(hit(Place::Left, 497.0, 400.0, w, h), Some(Hit::Edge));
        assert_eq!(hit(Place::Bottom, 250.0, 3.0, w, h), Some(Hit::Edge));
        assert_eq!(body(w, h), [20.0, 81.0, 460.0, 650.0]);
    }

    #[test]
    fn only_https_and_our_page_load() {
        let own = "ocsheet://localhost/";
        assert!(allowed("https://lens.google.com/search?p=x", own));
        assert!(allowed(own, own));
        for bad in [
            "file:///etc/passwd",
            "http://example.com/",
            "data:text/html,x",
            "javascript:1",
            "ocsheet://localhost/other",
        ] {
            assert!(!allowed(bad, own), "{bad}");
        }
    }

    #[test]
    fn logs_drop_the_query() {
        assert_eq!(
            short("https://www.google.com/search?udm=50&q=secret#x"),
            "https://www.google.com/search"
        );
        assert_eq!(short("https://a.b/c#q=x"), "https://a.b/c");
    }

    #[test]
    fn lines_round_trip_and_chatter_is_ignored() {
        for l in [
            Line::Ready,
            Line::Open("https://a.io/x?y=1".into()),
            Line::Size(None),
            Line::Size(Some((640.0, 480.0))),
        ] {
            assert_eq!(Line::parse(&l.text()), Some(l.clone()), "{l:?}");
        }
        assert_eq!(Line::parse("ready\r\n"), Some(Line::Ready));
        for junk in [
            "",
            "readyx",
            "open ",
            "open a b",
            "size 1",
            "size x 2",
            "size 0 5",
            "size -1 5",
            "size inf 5",
            "Gtk-WARNING: whatever",
            "close",
        ] {
            assert_eq!(Line::parse(junk), None, "{junk:?}");
        }
    }
}
