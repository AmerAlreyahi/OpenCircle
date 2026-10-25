//! What the app (`src/sheet.rs`) and the results-card helper (`opencircle-sheet`)
//! share: the helper's command line, the lines it prints, the card's geometry
//! and the page filter. Plain std only: the helper includes this file, so do
//! the app's tests.
//!
//! App → helper: `opencircle-sheet <lens|ai> [--side left|right] [--size WxH]
//! --colours BG,BORDER,HOVER,TEXT,MUTED --shadow A`, then on stdin the page
//! (Lens: the auto-submit HTML, from memory, never a file) or the AI Mode URL.
//! Helper → app, one per line on stdout: `ready` (shown), `open <url>` (to the
//! user's browser), `leave <url>` (the user took the page to the browser: the
//! card closed), `size W H` / `size default` (what the user resized it to),
//! `side left|right` (where the user docked it).

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
    /// The side the card sits on (the user's choice: Left or Right).
    pub side: Place,
    /// The size the user last resized the card to.
    pub size: Option<(f64, f64)>,
    /// 0xRRGGBB: background, border, hover (chip, buttons), text, muted.
    pub colours: [u32; 5],
    /// The shadow's opacity.
    pub shadow: f64,
}

impl Args {
    pub fn to_vec(&self) -> Vec<String> {
        let mut v = vec![self.kind.arg().to_string()];
        v.extend(["--side".into(), self.side.arg().into()]);
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
            side: Place::Right,
            size: None,
            colours: [0x1c1c1e, 0x3a3a3c, 0x2c2c2e, 0xffffff, 0x8e8e93],
            shadow: 0.35,
        };
        let mut it = args[1..].iter();
        while let Some(flag) = it.next() {
            let value = it.next().ok_or(format!("{flag}: no value"))?;
            let bad = || format!("{flag}: bad value {value:?}");
            match flag.as_str() {
                "--side" => a.side = Place::side(value).ok_or_else(bad)?,
                "--size" => {
                    let (w, h) = value.split_once('x').ok_or_else(bad)?;
                    let (w, h): (f64, f64) =
                        (w.parse().map_err(|_| bad())?, h.parse().map_err(|_| bad())?);
                    a.size = Some((w, h)).filter(|&s| size_ok(s));
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

/// A card size the user gave: (width, height), 0 = that one is the default.
fn size_ok((w, h): (f64, f64)) -> bool {
    w >= 0.0 && h >= 0.0 && w.is_finite() && h.is_finite() && (w > 0.0 || h > 0.0)
}

/// The side the card hugs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Left,
    Right,
    Bottom,
}

impl Place {
    /// "left" / "right": the sides the user can choose (`--side`, `side …`).
    pub fn side(s: &str) -> Option<Place> {
        match s {
            "left" => Some(Place::Left),
            "right" => Some(Place::Right),
            _ => None,
        }
    }

    pub fn arg(self) -> &'static str {
        match self {
            Place::Left => "left",
            Place::Right | Place::Bottom => "right",
        }
    }
}

/// The card in an `area` (w, h), as `ui::widgets::card::place` lays a card
/// out: on the user's `side` (owner, 2026-10-24: left or right by preference,
/// default right), full height, `SHARE` of the width (at least `WIDTH`);
/// narrow or portrait: at the bottom, centred. `size` (the user's last
/// resize, kept across runs) sets the width of a side card, the height of a
/// bottom one.
pub fn place(area: (f64, f64), side: Place, size: Option<(f64, f64)>) -> (Place, Rect4) {
    let (w, h) = area;
    let inner = ((w - 2.0 * GAP).max(0.0), (h - 2.0 * GAP).max(0.0));
    let fit = |v: f64, min: f64, max: f64| v.clamp(min.min(max), max);
    if w < NARROW || h > w {
        let width = inner.0.min(BOTTOM_WIDTH);
        let height = match size {
            Some((_, sh)) if sh > 0.0 => fit(sh, MIN.1, inner.1),
            _ => (h / 2.0).max(BOTTOM_MIN).min(inner.1),
        };
        let r = [(w - width) / 2.0, h - GAP - height, width, height];
        return (Place::Bottom, r);
    }
    let width = match size {
        Some((sw, _)) if sw > 0.0 => fit(sw, MIN.0, inner.0),
        _ => (w * SHARE).max(WIDTH).min(inner.0),
    };
    if side == Place::Left {
        (Place::Left, [GAP, GAP, width, inner.1])
    } else {
        (Place::Right, [w - GAP - width, GAP, width, inner.1])
    }
}

/// How far (logical px) a card may overlap the selection before it moves.
const COVER_SLACK: f64 = 8.0;

/// The side this search's card opens on (owner, 2026-10-25): the user's
/// `side`, unless there it would cover the selection's left and right edges
/// `sel` by more than a few px and the other side would not; then the other
/// side, for this search only (the saved side stays). Neither side free (a
/// huge selection), narrow screens (bottom card) or no selection: `side`.
pub fn side_for(
    area: (f64, f64),
    side: Place,
    size: Option<(f64, f64)>,
    sel: Option<(f64, f64)>,
) -> Place {
    let Some((x0, x1)) = sel else { return side };
    let covers = |p: Place| match place(area, p, size) {
        (Place::Bottom, _) => false,
        (_, [x, _, w, _]) => x1.min(x + w) - x0.max(x) > COVER_SLACK,
    };
    let other = match side {
        Place::Left => Place::Right,
        _ => Place::Left,
    };
    if covers(side) && !covers(other) {
        other
    } else {
        side
    }
}

/// The side a card dropped at `r` docks to: the nearer one (its centre on
/// the left half: left).
pub fn dock(area: (f64, f64), r: Rect4) -> Place {
    if r[0] + r[2] / 2.0 < area.0 / 2.0 {
        Place::Left
    } else {
        Place::Right
    }
}

/// What of a resized card's `size` is kept for next time: a side card's
/// width, a bottom card's height (0 = not that one).
pub fn kept(p: Place, (w, h): (f64, f64)) -> (f64, f64) {
    if p == Place::Bottom {
        (0.0, h)
    } else {
        (w, 0.0)
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
    /// History (shown only when the user keeps one).
    History,
    /// The inner edge: drag to resize, double-click for the default size.
    Edge,
    /// The header (not a button): drag to move the card; it docks to the
    /// nearer side on release. A bottom card doesn't move.
    Head,
}

/// The × button's centre in a `w` wide card.
pub fn close_at(w: f64) -> (f64, f64) {
    (w - 12.0 - 22.0, 8.0 + 22.0)
}

/// The "Open in browser" button's centre: left of ×.
pub fn browser_at(w: f64) -> (f64, f64) {
    (w - 12.0 - 44.0 - 4.0 - 22.0, 8.0 + 22.0)
}

/// The History button's centre: left of "Open in browser".
pub fn history_at(w: f64) -> (f64, f64) {
    (w - 12.0 - 2.0 * (44.0 + 4.0) - 22.0, 8.0 + 22.0)
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
    } else if on(history_at(w)) {
        Some(Hit::History)
    } else if edge {
        Some(Hit::Edge)
    } else if y < HEADER && p != Place::Bottom {
        Some(Hit::Head)
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

/// What may load in the card (a page or a frame: WebKitGTK and WebView2 ask
/// about navigations only, never about a page's images, scripts or styles):
/// Google's own Lens / Search pages over https (`google_host`), and our
/// in-memory pages (`own`). Never file://, data:, javascript:, plain http,
/// another site, or Google's user-content / static hosts (googleusercontent,
/// ggpht, gstatic: anyone's files, a phishing page in a trusted card): those
/// go to the user's browser on a click, or nowhere.
pub fn allowed(url: &str, own: &str) -> bool {
    if url == own || is_history(url, own) || url == "about:blank" {
        return true;
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    google_host(&rest[..end])
}

/// The hosts Lens and AI Mode pages come from (seen on both VMs: the Lens
/// upload, its redirect to Search, the consent page, the "unusual traffic"
/// check and its reCAPTCHA frame): lens.google.com, ipv4. / ipv6.google.com
/// (the check), and google.<tld> (google.com, google.de, google.co.uk,
/// google.com.br) bare or with www. / consent. Exact ASCII labels only: no
/// port, userinfo, punycode look-alike, other subdomain or
/// "google.com.evil.example".
pub fn google_host(host: &str) -> bool {
    if matches!(
        host,
        "lens.google.com" | "ipv4.google.com" | "ipv6.google.com"
    ) {
        return true;
    }
    let host = (host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("consent.")))
    .unwrap_or(host);
    let Some(tld) = host.strip_prefix("google.") else {
        return false;
    };
    let cc = |l: &str| l.len() == 2 && l.bytes().all(|b| b.is_ascii_lowercase());
    match tld.split_once('.') {
        None => tld == "com" || cc(tld),
        Some((second, cc_)) => matches!(second, "co" | "com") && cc(cc_),
    }
}

/// What the card does with a navigation or a new window the page asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Go {
    /// Load it in the card.
    Here,
    /// Not in the card: the user's browser (a link the user clicked).
    Browser,
    /// Nowhere (not the user's doing, too many, or not a web link).
    Drop,
}

/// Links the card hands to the browser: at most one a second, ten a card.
#[derive(Debug, Default)]
pub struct Limit {
    sent: u32,
    last: Option<std::time::Instant>,
}

impl Limit {
    pub const MAX: u32 = 10;
    const GAP: std::time::Duration = std::time::Duration::from_secs(1);

    /// One more link to the browser at `now`, if within the limit.
    pub fn take(&mut self, now: std::time::Instant) -> bool {
        let spaced = self.last.is_none_or(|t| now.duration_since(t) >= Self::GAP);
        let ok = spaced && self.sent < Self::MAX;
        if ok {
            self.sent += 1;
            self.last = Some(now);
        }
        ok
    }
}

/// Where `url` goes. `new_window`: the page asked for a new window (never one
/// in the card); `gesture`: the user's own click or key started it.
pub fn go(
    url: &str,
    own: &str,
    new_window: bool,
    gesture: bool,
    limit: &mut Limit,
    now: std::time::Instant,
) -> Go {
    if !new_window && allowed(url, own) {
        return Go::Here;
    }
    let web = url.starts_with("https://") || url.starts_with("http://");
    if gesture && web && limit.take(now) {
        Go::Browser
    } else {
        Go::Drop
    }
}

/// The History page's path under our own page.
pub const HISTORY: &str = "history";
/// The page message that clears the history (only from our History page).
pub const CLEAR: &str = "clear-history";

/// Our History page (`own` + "history"): the only page that may clear it.
pub fn is_history(url: &str, own: &str) -> bool {
    url.strip_prefix(own) == Some(HISTORY)
}

/// The app's stdin: the page (Lens HTML / AI Mode URL), then, when the user
/// keeps a history, NUL and the History page.
pub fn split_input(input: &str) -> (&str, Option<&str>) {
    match input.split_once('\0') {
        Some((page, history)) => (page, Some(history)),
        None => (input, None),
    }
}

/// Google's "unusual traffic" check (`https://<…>google.com/sorry/…`): the card
/// shows a hint over it; only the user ever answers it.
pub fn is_captcha(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    (host == "google.com" || host.ends_with(".google.com")) && path.starts_with("/sorry/")
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
    /// The user took the page to the browser ("Open in browser"): the card
    /// closed, the app opens it and gets out of the way.
    Leave(String),
    /// The user docked the card on this side (Left or Right).
    Side(Place),
    /// The user resized the card (`None`: back to the default size).
    Size(Option<(f64, f64)>),
    /// A results page finished loading in the card (the history keeps Lens's).
    At(String),
    /// The user pressed Clear on the History page.
    ClearHistory,
}

impl Line {
    /// Anything else (the web engine's own chatter) is not a line for the app.
    pub fn parse(line: &str) -> Option<Line> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "ready" {
            return Some(Line::Ready);
        }
        if line == CLEAR {
            return Some(Line::ClearHistory);
        }
        let link = |url: &str| !url.is_empty() && !url.contains(char::is_whitespace);
        if let Some(url) = line.strip_prefix("open ") {
            return link(url).then(|| Line::Open(url.into()));
        }
        if let Some(url) = line.strip_prefix("at ") {
            return link(url).then(|| Line::At(url.into()));
        }
        if let Some(url) = line.strip_prefix("leave ") {
            return link(url).then(|| Line::Leave(url.into()));
        }
        if let Some(side) = line.strip_prefix("side ") {
            return Place::side(side).map(Line::Side);
        }
        let size = line.strip_prefix("size ")?;
        if size == "default" {
            return Some(Line::Size(None));
        }
        let (w, h) = size.split_once(' ')?;
        let (w, h): (f64, f64) = (w.parse().ok()?, h.parse().ok()?);
        size_ok((w, h)).then_some(Line::Size(Some((w, h))))
    }

    pub fn text(&self) -> String {
        match self {
            Line::Ready => "ready".into(),
            Line::Open(url) => format!("open {url}"),
            Line::At(url) => format!("at {url}"),
            Line::Leave(url) => format!("leave {url}"),
            Line::Side(p) => format!("side {}", p.arg()),
            Line::ClearHistory => CLEAR.into(),
            Line::Size(None) => "size default".into(),
            Line::Size(Some((w, h))) => format!("size {} {}", w.round(), h.round()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_opens_off_the_selection_for_this_search_only() {
        let area = (1716.0, 1271.0); // the Windows VM; a right card spans 1010..1696
        let on = |side, sel| side_for(area, side, None, Some(sel));
        // the right-hand picture circled, saved right: left this time
        assert_eq!(on(Place::Right, (1015.0, 1235.0)), Place::Left);
        // middle-left text, saved right: right (a touch of overlap is fine)
        assert_eq!(on(Place::Right, (470.0, 1015.0)), Place::Right);
        // saved left, selection on the left: right
        assert_eq!(on(Place::Left, (100.0, 400.0)), Place::Right);
        // huge selection (neither side free), no selection, narrow screen: saved side
        assert_eq!(on(Place::Right, (100.0, 1600.0)), Place::Right);
        assert_eq!(side_for(area, Place::Right, None, None), Place::Right);
        assert_eq!(
            side_for((800.0, 600.0), Place::Right, None, Some((500.0, 700.0))),
            Place::Right
        );
        // the user's width counts
        let wide = Some((1200.0, 0.0));
        assert_eq!(
            side_for(area, Place::Right, wide, Some((600.0, 700.0))),
            Place::Right
        );
    }

    #[test]
    fn args_round_trip_and_reject_junk() {
        let a = Args {
            kind: Kind::Ai,
            side: Place::Left,
            size: Some((700.0, 900.0)),
            colours: [0x112233, 0x445566, 0x778899, 0xaabbcc, 0xddeeff],
            shadow: 0.5,
        };
        assert_eq!(Args::parse(&a.to_vec()), Ok(a));
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let lens = Args::parse(&s(&["lens"])).unwrap();
        assert_eq!(
            (lens.kind, lens.side),
            (Kind::Lens, Place::Right),
            "default right"
        );
        for bad in [
            &["x"][..],
            &[],
            &["ai", "--side"],
            &["ai", "--side", "bottom"],
            &["ai", "--side", "Left"],
            &["ai", "--size", "10"],
            &["ai", "--colours", "12345,1,2,3,4"],
            &["ai", "--colours", "112233"],
            &["ai", "--evil", "1"],
        ] {
            assert!(Args::parse(&s(bad)).is_err(), "{bad:?}");
        }
        // out of range: ignored, not trusted
        let a = Args::parse(&s(&["ai", "--size", "-1x5"])).unwrap();
        assert_eq!(a.size, None);
    }

    #[test]
    fn card_sits_on_the_users_side_with_the_remembered_width() {
        let area = (1920.0, 1080.0);
        let right = [1132.0, 20.0, 768.0, 1040.0];
        assert_eq!(place(area, Place::Right, None), (Place::Right, right));
        assert_eq!(
            place(area, Place::Left, None),
            (Place::Left, [20.0, 20.0, 768.0, 1040.0])
        );
        // A remembered size: the width, on either side.
        let mine = Some((500.0, 300.0));
        assert_eq!(
            place(area, Place::Left, mine),
            (Place::Left, [20.0, 20.0, 500.0, 1040.0])
        );
        assert_eq!(
            place(area, Place::Right, mine),
            (Place::Right, [1400.0, 20.0, 500.0, 1040.0])
        );
        // ... within bounds
        assert_eq!(place(area, Place::Right, Some((10.0, 1.0))).1[2], MIN.0);
        assert_eq!(place(area, Place::Right, Some((5000.0, 1.0))).1[2], 1880.0);
        // Portrait or narrow: bottom, centred; a remembered height.
        let (p, r) = place((1080.0, 1920.0), Place::Left, None);
        assert_eq!((p, r), (Place::Bottom, [220.0, 940.0, 640.0, 960.0]));
        let (p, r) = place((800.0, 600.0), Place::Right, Some((999.0, 400.0)));
        assert_eq!((p, r), (Place::Bottom, [80.0, 180.0, 640.0, 400.0]));
    }

    #[test]
    fn a_dropped_card_docks_to_the_nearer_side() {
        let area = (1920.0, 1080.0);
        let at = |x: f64| [x, 300.0, 500.0, 700.0];
        assert_eq!(dock(area, at(20.0)), Place::Left);
        assert_eq!(dock(area, at(700.0)), Place::Left, "centre 950 < 960");
        assert_eq!(dock(area, at(711.0)), Place::Right, "centre 961");
        assert_eq!(dock(area, at(1400.0)), Place::Right);
        assert_eq!(dock(area, at(-300.0)), Place::Left, "dragged off the left");
        assert_eq!(dock(area, at(2500.0)), Place::Right);
        // A resize keeps the side card's width, the bottom card's height.
        assert_eq!(kept(Place::Left, (500.0, 1040.0)), (500.0, 0.0));
        assert_eq!(kept(Place::Bottom, (640.0, 400.0)), (0.0, 400.0));
    }

    #[test]
    fn the_inner_edge_resizes_within_bounds() {
        let area = (1920.0, 1080.0);
        let (p, r) = place(area, Place::Right, None);
        assert_eq!(
            resized(p, r, -100.0, 0.0, area),
            [1032.0, 20.0, 868.0, 1040.0]
        );
        assert_eq!(resized(p, r, 900.0, 0.0, area)[2], MIN.0);
        assert_eq!(resized(p, r, -5000.0, 0.0, area)[0], GAP);
        let (p, r) = place(area, Place::Left, None);
        assert_eq!(resized(p, r, 100.0, 0.0, area), [20.0, 20.0, 868.0, 1040.0]);
        assert_eq!(resized(p, r, 5000.0, 0.0, area)[2], 1880.0);
        let (p, r) = place((800.0, 600.0), Place::Right, None);
        assert_eq!(resized(p, r, 0.0, -50.0, (800.0, 600.0))[3], 350.0);
        assert_eq!(resized(p, r, 0.0, 500.0, (800.0, 600.0))[3], MIN.1);
    }

    #[test]
    fn buttons_the_inner_edge_and_the_header_grip() {
        let (w, h) = (500.0, 800.0);
        let (cx, cy) = close_at(w);
        assert_eq!(hit(Place::Right, cx, cy, w, h), Some(Hit::Close));
        let (bx, by) = browser_at(w);
        assert_eq!(hit(Place::Right, bx, by, w, h), Some(Hit::Browser));
        let (hx, hy) = history_at(w);
        assert_eq!(hit(Place::Right, hx, hy, w, h), Some(Hit::History));
        // The header moves a side card (buttons win); a bottom card stays.
        assert_eq!(hit(Place::Right, 200.0, 20.0, w, h), Some(Hit::Head));
        assert_eq!(hit(Place::Left, 200.0, 60.0, w, h), Some(Hit::Head));
        assert_eq!(hit(Place::Bottom, 200.0, 20.0, w, h), None);
        assert_eq!(hit(Place::Right, 250.0, 400.0, w, h), None); // the page's
        assert_eq!(hit(Place::Right, 3.0, 400.0, w, h), Some(Hit::Edge));
        assert_eq!(hit(Place::Right, 3.0, 790.0, w, h), None); // a corner: not the edge
        assert_eq!(hit(Place::Right, 497.0, 400.0, w, h), None); // outer edge: not a handle
        assert_eq!(hit(Place::Left, 497.0, 400.0, w, h), Some(Hit::Edge));
        assert_eq!(hit(Place::Bottom, 250.0, 3.0, w, h), Some(Hit::Edge));
        assert_eq!(body(w, h), [20.0, 81.0, 460.0, 650.0]);
    }

    #[test]
    fn only_googles_pages_and_ours_load_in_the_card() {
        let own = "ocsheet://localhost/";
        for ok in [
            "https://lens.google.com/search?p=x",
            "https://www.google.com/search?udm=50&q=x",
            "https://www.google.de/search?q=x",
            "https://google.co.uk/",
            "https://www.google.com.br/x",
            "https://consent.google.fr/ml",
            "https://consent.google.com/ml",
            "https://google.com/",
            "https://www.google.com/sorry/index?continue=x",
            "https://ipv4.google.com/sorry/index",
            "https://www.google.com/recaptcha/enterprise/anchor",
            own,
        ] {
            assert!(allowed(ok, own), "{ok}");
        }
        for bad in [
            "https://google.com.evil.com/",
            "https://www.google.com.evil.com/search",
            "https://evil.com/google.com",
            "https://evil.com/?u=https://www.google.com/",
            "https://xn--googl-fsa.com/",
            "https://googlé.com/",
            "https://notgoogle.com/",
            "https://evilgoogle.com/",
            "https://google.evil/",
            "https://google.co.evil.uk/",
            "https://www.google.com:8443/",
            "https://user@www.google.com/",
            "https://GOOGLE.com/",
            "https://en.wikipedia.org/wiki/Lighthouse",
            "http://www.google.com/",
            "https://ep2.adtrafficquality.google/sodar",
            // Google's user-content / static hosts: anyone's files, never a page here.
            "https://lh3.googleusercontent.com/a",
            "https://evil-site.googleusercontent.com/login.html",
            "https://yt3.ggpht.com/a",
            "https://www.gstatic.com/recaptcha/x.js",
            "https://gstatic.com/x",
            // Other Google subdomains: not what Lens or AI Mode need.
            "https://sites.google.com/view/evil",
            "https://docs.google.com/forms/d/x",
            "https://accounts.google.com/x",
            "https://lens.google.com.evil.com/",
            "https://www.lens.google.com/",
            "https://www.www.google.com/",
            "https://google.comm/",
            "https://google.c0m/",
            "https://google.co.ukk/",
            "https://googIe.com/",
        ] {
            assert!(!allowed(bad, own), "{bad}");
        }
        for bad in [
            "file:///etc/passwd",
            "http://example.com/",
            "data:text/html,x",
            "javascript:1",
            "ocsheet://localhost/other",
            "ocsheet://localhost/history/x",
        ] {
            assert!(!allowed(bad, own), "{bad}");
        }
        assert!(allowed("ocsheet://localhost/history", own));
        // Only our History page may clear the history.
        assert!(is_history("ocsheet://localhost/history", own));
        for not in [
            own,
            "https://www.google.com/history",
            "https://evil.test/ocsheet://localhost/history",
        ] {
            assert!(!is_history(not, own), "{not}");
        }
        assert_eq!(split_input("https://x"), ("https://x", None));
        assert_eq!(split_input("<p>\0<h>"), ("<p>", Some("<h>")));
    }

    #[test]
    fn only_the_users_clicks_reach_the_browser_and_not_too_often() {
        use std::time::{Duration, Instant};
        let own = "ocsheet://localhost/";
        let t = Instant::now();
        let s = |n: u64| t + Duration::from_millis(n);
        let mut limit = Limit::default();
        let site = "https://en.wikipedia.org/wiki/Lighthouse";
        // Google's own pages load here; another site never does.
        let google = "https://www.google.com/search?q=x";
        assert_eq!(go(google, own, false, false, &mut limit, s(0)), Go::Here);
        // Not the user's doing (a script, an ad, a redirect chain): dropped.
        assert_eq!(go(site, own, false, false, &mut limit, s(0)), Go::Drop);
        assert_eq!(go(site, own, true, false, &mut limit, s(0)), Go::Drop);
        // Google's user-content hosts never load as a page in the card: a
        // redirect there is dropped, a click goes to the browser.
        let user = "https://evil-site.googleusercontent.com/login.html";
        assert_eq!(go(user, own, false, false, &mut limit, s(0)), Go::Drop);
        let mut once = Limit::default();
        assert_eq!(go(user, own, false, true, &mut once, s(0)), Go::Browser);
        // A click: the browser, at most once a second ...
        assert_eq!(go(site, own, true, true, &mut limit, s(0)), Go::Browser);
        assert_eq!(go(site, own, false, true, &mut limit, s(500)), Go::Drop);
        assert_eq!(go(site, own, false, true, &mut limit, s(1000)), Go::Browser);
        // ... new windows never open in the card, even Google's
        assert_eq!(
            go(google, own, true, true, &mut limit, s(2000)),
            Go::Browser
        );
        // ... and ten a card.
        for i in 3..10 {
            assert_eq!(
                go(site, own, true, true, &mut limit, s(i * 1000)),
                Go::Browser
            );
        }
        assert_eq!(go(site, own, true, true, &mut limit, s(60_000)), Go::Drop);
        // Never a non-web link, even on a click.
        let mut fresh = Limit::default();
        for bad in [
            "file:///etc/passwd",
            "javascript:1",
            "mailto:a@b.c",
            "data:,x",
        ] {
            assert_eq!(
                go(bad, own, true, true, &mut fresh, s(0)),
                Go::Drop,
                "{bad}"
            );
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
            Line::Leave("https://lens.google.com/search?p=1".into()),
            Line::Side(Place::Left),
            Line::Side(Place::Right),
            Line::Size(None),
            Line::Size(Some((640.0, 480.0))),
            Line::Size(Some((640.0, 0.0))),
            Line::At("https://lens.google.com/search?p=1".into()),
            Line::ClearHistory,
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
            "size 0 0",
            "size -1 5",
            "size inf 5",
            "side bottom",
            "side ",
            "leave ",
            "Gtk-WARNING: whatever",
            "close",
        ] {
            assert_eq!(Line::parse(junk), None, "{junk:?}");
        }
    }
}
