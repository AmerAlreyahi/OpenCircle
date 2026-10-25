//! Windows: a winit window, the web view (WebView2) as its child. The frame
//! (header, footer, border, grip) is painted with softbuffer, its text with GDI.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, SIZE};
use windows::Win32::Graphics::Gdi::{
    AddFontMemResourceEx, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateRoundRectRgn,
    DEFAULT_CHARSET, DeleteObject, FW_NORMAL, GetDC, GetTextExtentPoint32W, HDC, HFONT,
    OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
    TRANSPARENT, TextOutW,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::core::{PCWSTR, w};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{CursorIcon, ResizeDirection, Window, WindowId, WindowLevel};
use wry::{Rect, WebContext, WebView, WebViewExtWindows};

use super::proto::{
    self, Args, BODY_PAD, BODY_R, FOOTER, HEADER, Hit, Kind, Line, MIN, Place, RADIUS, Rect4,
};
use super::{CHIP, NO_RUNTIME, Page, builder, data_dir, history_url, lerp, log, say, slid};

/// The human-check hint strip's height (logical px).
const HINT: f64 = 44.0;
/// The longest the card waits hidden for Google's page to start drawing
/// (the app shows its loading card in this very place meanwhile).
const SHOW_CAP: Duration = Duration::from_secs(4);
/// A resize is kept this long after the card stops changing size (the app
/// can end the helper any time after: an outside click, Esc in the overlay).
const KEEP_AFTER: Duration = Duration::from_millis(300);

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

/// `history`: the History page (the user keeps one: its button shows).
pub fn run(args: &Args, page: Page, history: Option<String>, t0: Instant) -> u8 {
    if let Err(e) = wry::webview_version() {
        eprintln!("[sheet] no WebView2: {e}");
        return NO_RUNTIME;
    }
    // The app's icon font, for the header's icon (the same glyph as the app's card).
    let icons = crate::ui::widgets::ICON_FONT_BYTES;
    let mut fonts = 0u32;
    // SAFETY: the bytes are static; the font stays this process's until it exits.
    let _ = unsafe {
        AddFontMemResourceEx(icons.as_ptr().cast(), icons.len() as u32, None, &mut fonts)
    };
    let Ok(el) = winit::event_loop::EventLoop::<Ev>::with_user_event().build() else {
        return 1;
    };
    let mut app = App {
        kind: args.kind,
        has_history: history.is_some(),
        history,
        side: args.side,
        size: args.size,
        colours: args.colours,
        page: Some(page),
        t0,
        proxy: el.create_proxy(),
        ctx: WebContext::new(Some(data_dir())),
        win: None,
        view: None,
        surface: None,
        at: Place::Right,
        target: [0.0; 4],
        screen: (0.0, 0.0),
        mon: (0, 0),
        k: 1.0,
        cursor: (0.0, 0.0),
        hover: false,
        last_edge_press: None,
        resized: false,
        keep: None,
        pending: None,
        grab: None,
        dock: None,
        shown: false,
        captcha: false,
        code: 0,
    };
    if el.run_app(&mut app).is_err() {
        return 1;
    }
    app.code
}

struct App {
    kind: Kind,
    /// The History page, until the web view takes it; its button shows.
    history: Option<String>,
    has_history: bool,
    /// The user's side (Left or Right); the card docks there.
    side: Place,
    /// The size to open at (the user's last resize), then the one to report.
    size: Option<(f64, f64)>,
    colours: [u32; 5],
    page: Option<Page>,
    t0: Instant,
    proxy: EventLoopProxy<Ev>,
    ctx: WebContext,
    win: Option<Rc<Window>>,
    view: Option<WebView>,
    surface: Option<Surface>,
    at: Place,
    /// Final card rect (logical, monitor-relative).
    target: Rect4,
    /// Monitor size (logical).
    screen: (f64, f64),
    /// Monitor origin (physical).
    mon: (i32, i32),
    k: f64,
    /// Last cursor position in the card (logical).
    cursor: (f64, f64),
    /// The cursor is on the inner edge (the grip shows).
    hover: bool,
    last_edge_press: Option<Instant>,
    /// The user resized it (or reset it): tell the app (`keep`, and at the end).
    resized: bool,
    /// The size last changed then: told to the app `KEEP_AFTER` later.
    keep: Option<Instant>,
    /// Built, hidden until Google's page starts drawing (or `SHOW_CAP`).
    pending: Option<Instant>,
    /// The header is held: where in the card (logical); the card follows the pointer.
    grab: Option<(f64, f64)>,
    /// Docking after a drop: since when, and from where (to `target`).
    dock: Option<(Instant, Rect4)>,
    shown: bool,
    /// Google's human check is showing: the hint strip sits above the page.
    captcha: bool,
    code: u8,
}

/// What the web view tells the window (from its own callbacks).
enum Ev {
    /// The page's "close".
    Close,
    /// Google's page started drawing: the card shows.
    Content,
    /// Google's human check showed (true) or went (false).
    Captcha(bool),
}

/// The hint strip above the page (logical, card-relative) and its button.
fn hint_rects(w: f64) -> (Rect4, Rect4) {
    let strip = [BODY_PAD, HEADER + BODY_PAD, w - 2.0 * BODY_PAD, HINT];
    let button = [
        strip[0] + strip[2] - 6.0 - 132.0,
        strip[1] + 6.0,
        132.0,
        HINT - 12.0,
    ];
    (strip, button)
}

fn inside([x, y, w, h]: Rect4, (px, py): (f64, f64)) -> bool {
    (x..x + w).contains(&px) && (y..y + h).contains(&py)
}

fn hwnd(w: &Window) -> Option<HWND> {
    match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// Distance from (x, y) to the edge of a rounded rect (< 0 inside).
fn rounded(x: f64, y: f64, [ox, oy, w, h]: Rect4, r: f64) -> f64 {
    let qx = (x - ox - w / 2.0).abs() - (w / 2.0 - r);
    let qy = (y - oy - h / 2.0).abs() - (h / 2.0 - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

fn mix(a: u32, b: u32, t: f64) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f64, ((b >> s) & 255) as f64);
        ((x + (y - x) * t.clamp(0.0, 1.0)).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// A GDI font (Segoe UI) at `px` physical pixels.
fn font(px: f64) -> HFONT {
    font_of(px, w!("Segoe UI"))
}

/// The header's icon: the app's own (lucide, as on the app's card).
fn glyph(kind: Kind) -> String {
    match kind {
        Kind::Lens => crate::actions::lens::ACTION.icon,
        Kind::Ai => crate::actions::ask::ACTION.icon,
    }
    .to_string()
}

fn font_of(px: f64, face: PCWSTR) -> HFONT {
    // SAFETY: plain GDI call; the caller deletes the font.
    unsafe {
        CreateFontW(
            -(px as i32),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            face,
        )
    }
}

/// Draws `text` at (x, y) (physical, top-left).
fn text(dc: HDC, f: HFONT, color: u32, x: i32, y: i32, text: &str) {
    let t: Vec<u16> = text.encode_utf16().collect();
    // SAFETY: `dc` is a live DC; `t` outlives the calls.
    unsafe {
        let old = SelectObject(dc, f.into());
        // COLORREF is 0x00BBGGRR.
        let c = ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff);
        SetTextColor(dc, COLORREF(c));
        let _ = TextOutW(dc, x, y, &t);
        SelectObject(dc, old);
    }
}

fn width_of(dc: HDC, f: HFONT, text: &str) -> i32 {
    let t: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    // SAFETY: as in `text`.
    unsafe {
        let old = SelectObject(dc, f.into());
        let _ = GetTextExtentPoint32W(dc, &t, &mut size);
        SelectObject(dc, old);
    }
    size.cx
}

impl App {
    fn phys(&self, x: f64, y: f64) -> PhysicalPosition<i32> {
        PhysicalPosition::new(
            self.mon.0 + (x * self.k) as i32,
            self.mon.1 + (y * self.k) as i32,
        )
    }

    fn logical(&self) -> (f64, f64) {
        let s = (self.win.as_ref())
            .map(|w| w.inner_size())
            .unwrap_or_default();
        (s.width as f64 / self.k, s.height as f64 / self.k)
    }

    fn view_rect(&self, size: PhysicalSize<u32>) -> Rect {
        let k = self.k;
        let [x, mut y, w, mut h] = proto::body(size.width as f64 / k, size.height as f64 / k);
        if self.captcha {
            // the hint strip and a gap above the page
            (y, h) = (y + HINT + 12.0, (h - HINT - 12.0).max(0.0));
        }
        Rect {
            position: PhysicalPosition::new((x * k) as i32, (y * k) as i32).into(),
            size: PhysicalSize::new((w * k) as u32, (h * k) as u32).into(),
        }
    }

    /// Clip the window to the rounded card and the page to its rounded body
    /// (regions are aliased, and drop the DWM shadow on Windows 10).
    fn round(&self) {
        let Some(w) = &self.win else { return };
        let (Some(h), s, k) = (hwnd(w), w.inner_size(), self.k) else {
            return;
        };
        let r = (2.0 * RADIUS * k) as i32;
        // SAFETY: plain GDI/user32 calls; each window owns its region afterwards.
        unsafe {
            let rgn = CreateRoundRectRgn(0, 0, s.width as i32 + 1, s.height as i32 + 1, r, r);
            let _ = SetWindowRgn(h, Some(rgn), true);
            if let Some(v) = &self.view {
                let b = self.view_rect(s);
                let bs = b.size.to_physical::<u32>(1.0);
                let br = (2.0 * BODY_R * k) as i32;
                let rgn =
                    CreateRoundRectRgn(0, 0, bs.width as i32 + 1, bs.height as i32 + 1, br, br);
                let _ = SetWindowRgn(v.hwnd(), Some(rgn), true);
            }
        }
    }

    fn paint(&mut self) {
        let (k, at, hover, kind) = (self.k, self.at, self.hover, self.kind);
        let [bg, border, chip_bg, ink, muted] = self.colours;
        let (Some(w), Some(s)) = (self.win.clone(), &mut self.surface) else {
            return;
        };
        let Some(h) = hwnd(&w) else { return };
        let size = w.inner_size();
        let (Some(nw), Some(nh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if s.resize(nw, nh).is_err() {
            return;
        }
        // SAFETY: our window's DC, released below.
        let dc = unsafe { GetDC(Some(h)) };
        let (f_title, f_small) = (font(16.0 * k), font(12.0 * k));
        let f_icon = font_of(16.0 * k, w!("lucide"));
        let (fw, fh) = (size.width as f64, size.height as f64);
        let glyph = glyph(kind);
        let icon_w = width_of(dc, f_icon, &glyph);
        let title_w = width_of(dc, f_title, kind.title());
        let chip_w = width_of(dc, f_small, CHIP) as f64;
        let x_title = 20.0 * k + icon_w as f64 + 10.0 * k;
        let chip = [
            x_title + title_w as f64 + 10.0 * k,
            ((HEADER - 1.0) * k - 22.0 * k) / 2.0,
            chip_w + 16.0 * k,
            22.0 * k,
        ];
        let pill = match at {
            Place::Right => [2.0 * k, fh / 2.0 - 20.0 * k, 4.0 * k, 40.0 * k],
            Place::Left => [fw - 6.0 * k, fh / 2.0 - 20.0 * k, 4.0 * k, 40.0 * k],
            Place::Bottom => [fw / 2.0 - 20.0 * k, 2.0 * k, 40.0 * k, 4.0 * k],
        };
        let card = [0.0, 0.0, fw, fh];
        let scaled = |r: Rect4| r.map(|v| v * k);
        let (strip, button) = hint_rects(fw / k);
        let (strip, button, captcha) = (scaled(strip), scaled(button), self.captcha);
        if let Ok(mut buf) = s.buffer_mut() {
            for y in 0..size.height {
                // the header's and the footer's rules
                let rule = y == ((HEADER - 1.0) * k) as u32 || y == (fh - FOOTER * k) as u32;
                for x in 0..size.width {
                    let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                    // 1 px border just inside the (region-clipped) edge.
                    let d = rounded(cx, cy, card, RADIUS * k);
                    let mut c = mix(bg, border, 1.0 - (-d - k).abs().min(1.0));
                    if rule && d < -k {
                        c = border;
                    }
                    c = mix(c, chip_bg, 0.5 - rounded(cx, cy, chip, 11.0 * k));
                    if hover {
                        c = mix(c, muted, 0.5 - rounded(cx, cy, pill, 2.0 * k));
                    }
                    if captcha {
                        c = mix(c, chip_bg, 0.5 - rounded(cx, cy, strip, 12.0 * k));
                        c = mix(c, bg, 0.5 - rounded(cx, cy, button, button[3] / 2.0));
                    }
                    buf[(y * size.width + x) as usize] = c;
                }
            }
            let _ = buf.present();
        }
        // Text on top (GDI, ClearType; the child web view is clipped out).
        // SAFETY: `dc` is live until ReleaseDC; the fonts are deleted after use.
        unsafe {
            SetBkMode(dc, TRANSPARENT);
            let ty = ((HEADER - 1.0 - 22.0) / 2.0 * k) as i32;
            text(dc, f_icon, ink, (20.0 * k) as i32, ty, &glyph);
            text(dc, f_title, ink, x_title as i32, ty, kind.title());
            let (cx, cy) = ((chip[0] + 8.0 * k) as i32, (chip[1] + 3.0 * k) as i32);
            text(dc, f_small, muted, cx, cy, CHIP);
            let past = if self.has_history { "↺" } else { "" };
            for ((bx, by), glyph) in [
                (proto::close_at(fw / k), "✕"),
                (proto::browser_at(fw / k), "↗"),
                (proto::history_at(fw / k), past),
            ] {
                let (x, y) = (((bx - 6.0) * k) as i32, ((by - 10.0) * k) as i32);
                text(dc, f_title, ink, x, y, glyph);
            }
            let fy = (fh - (FOOTER * k + 16.0 * k) / 2.0) as i32;
            text(dc, f_small, muted, (20.0 * k) as i32, fy, kind.privacy());
            if captcha {
                let (x, y) = ((strip[0] + 14.0 * k) as i32, (strip[1] + 7.0 * k) as i32);
                text(
                    dc,
                    f_small,
                    ink,
                    x,
                    y,
                    "Google wants to check you're human.",
                );
                let y2 = y + (16.0 * k) as i32;
                text(
                    dc,
                    f_small,
                    ink,
                    x,
                    y2,
                    "Solve it here, or open in your browser.",
                );
                let label = "Open in browser";
                let lw = width_of(dc, f_small, label) as f64;
                let bx = (button[0] + (button[2] - lw) / 2.0) as i32;
                let by = (button[1] + (button[3] - 15.0 * k) / 2.0) as i32;
                text(dc, f_small, ink, bx, by, label);
            }
            for f in [f_title, f_small, f_icon] {
                let _ = DeleteObject(f.into());
            }
            ReleaseDC(Some(h), dc);
        }
    }

    fn is_foreground(&self) -> bool {
        let ours = self.win.as_deref().and_then(hwnd);
        // SAFETY: plain Win32 call, no arguments.
        ours.is_some_and(|h| unsafe { GetForegroundWindow() } == h)
    }

    /// Ends the event loop; tells the app the size the user left it at.
    fn close(&mut self, el: &ActiveEventLoop) {
        if self.resized {
            self.say_size();
        }
        el.exit();
    }

    /// Tells the app the size the user gave the card (`None` = the default).
    fn say_size(&self) {
        let (w, h) = self.logical();
        say(Line::Size(self.size.map(|_| proto::kept(self.at, (w, h)))));
    }

    /// Google's page started drawing (or `SHOW_CAP` passed): the card shows,
    /// exactly over the app's loading card (no slide: it is already in place).
    fn show(&mut self) {
        if self.pending.take().is_none() {
            return;
        }
        if let Some(w) = &self.win {
            w.set_visible(true);
            w.focus_window();
        }
        log(self.t0, "shown", "");
        say(Line::Ready);
    }

    /// The current page to the user's browser (through the app), then close.
    fn open_in_browser(&mut self, el: &ActiveEventLoop) {
        if let Some(Ok(url)) = self.view.as_ref().map(WebView::url)
            && url.starts_with("https://")
        {
            say(Line::Leave(url));
        }
        self.close(el);
    }

    /// Back to the default size (double-click on the inner edge).
    fn reset(&mut self) {
        self.size = None;
        self.resized = true;
        let [x, y, w, h] = proto::place(self.screen, self.side, None).1;
        if let Some(win) = &self.win {
            win.set_outer_position(self.phys(x, y));
            let _ =
                win.request_inner_size(PhysicalSize::new((w * self.k) as u32, (h * self.k) as u32));
        }
    }
}

impl ApplicationHandler<Ev> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let Some(page) = self.page.take() else {
            return;
        };
        let Some(mon) = el
            .primary_monitor()
            .or_else(|| el.available_monitors().next())
        else {
            self.code = 1;
            el.exit();
            return;
        };
        let (mp, ms, k) = (mon.position(), mon.size(), mon.scale_factor());
        self.mon = (mp.x, mp.y);
        self.k = k;
        self.screen = (ms.width as f64 / k, ms.height as f64 / k);
        (self.at, self.target) = proto::place(self.screen, self.side, self.size);
        let [x, y, w, h] = self.target;
        let size = PhysicalSize::new((w * k) as u32, (h * k) as u32);
        let attrs = Window::default_attributes()
            .with_title("OpenCircle results")
            .with_decorations(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_skip_taskbar(true)
            .with_visible(false)
            .with_min_inner_size(LogicalSize::new(MIN.0, MIN.1))
            .with_position(self.phys(x, y))
            .with_inner_size(size);
        let Ok(win) = el.create_window(attrs) else {
            self.code = 1;
            el.exit();
            return;
        };
        let win = Rc::new(win);
        self.win = Some(win.clone());
        let (proxy, proxy_check) = (self.proxy.clone(), self.proxy.clone());
        let proxy_content = self.proxy.clone();
        let bounds = self.view_rect(size);
        let history = self.history.take();
        let view = builder(
            &mut self.ctx,
            page,
            history,
            self.t0,
            move || {
                let _ = proxy.send_event(Ev::Close);
            },
            move |on| {
                let _ = proxy_check.send_event(Ev::Captcha(on));
            },
            move || {
                let _ = proxy_content.send_event(Ev::Content);
            },
            self.colours[0],
        )
        .with_bounds(bounds)
        .build_as_child(&*win);
        match view {
            Ok(v) => {
                if let Err(e) = guard_links(&v, self.t0) {
                    eprintln!("[sheet] cannot guard links: {e}");
                    self.code = 1;
                    el.exit();
                    return;
                }
                self.view = Some(v);
            }
            Err(e) => {
                eprintln!("[sheet] web view: {e}");
                self.code = NO_RUNTIME;
                el.exit();
                return;
            }
        }
        self.round();
        self.surface = softbuffer::Context::new(win.clone())
            .and_then(|c| softbuffer::Surface::new(&c, win.clone()))
            .ok();
        // Hidden until Google's page draws: the app's loading card is there meanwhile.
        self.pending = Some(Instant::now());
        log(self.t0, "built", "");
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.pending.is_some_and(|t| t.elapsed() >= SHOW_CAP) {
            self.show();
        }
        if self.keep.is_some_and(|t| t.elapsed() >= KEEP_AFTER) {
            self.keep = None;
            self.say_size();
        }
        // Docking after a drop: where the card is now.
        if let Some((began, from)) = self.dock {
            let s = slid(began.elapsed());
            let r = lerp(from, self.target, s);
            if let Some(w) = &self.win {
                w.set_outer_position(self.phys(r[0], r[1]));
            }
            if s >= 1.0 {
                self.dock = None;
            }
        }
        let next = match self.dock {
            Some(_) => Some(Instant::now() + Duration::from_millis(8)),
            None => [
                self.pending.map(|t| t + SHOW_CAP),
                self.keep.map(|t| t + KEEP_AFTER),
            ]
            .into_iter()
            .flatten()
            .min(),
        };
        el.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: Ev) {
        match ev {
            Ev::Close => self.close(el),
            Ev::Content => self.show(),
            Ev::Captcha(on) if on != self.captcha => {
                self.captcha = on;
                log(
                    self.t0,
                    if on {
                        "human check shown"
                    } else {
                        "human check gone"
                    },
                    "",
                );
                if let (Some(v), Some(w)) = (&self.view, &self.win) {
                    let _ = v.set_bounds(self.view_rect(w.inner_size()));
                }
                self.round();
                self.paint();
            }
            Ev::Captcha(_) => {}
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.close(el),
            WindowEvent::Resized(size) => {
                if self.resized {
                    self.keep = Some(Instant::now());
                }
                if let Some(v) = &self.view {
                    let _ = v.set_bounds(self.view_rect(size));
                }
                self.round();
                self.paint();
            }
            WindowEvent::RedrawRequested => self.paint(),
            WindowEvent::Focused(true) => self.shown = true,
            // A click outside (the overlay or another app took the foreground).
            // Focus moving into the web view (our child) keeps us foreground.
            WindowEvent::Focused(false) if self.shown && !self.is_foreground() => self.close(el),
            // Only the header, footer and frame reach us (the page is a child).
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x / self.k, position.y / self.k);
                let (w, h) = self.logical();
                if let Some((gx, gy)) = self.grab {
                    // The card follows the pointer (winit holds the mouse
                    // capture while the button is down); it docks on release.
                    let [x, y, ..] = self.target;
                    self.target = [x + self.cursor.0 - gx, y + self.cursor.1 - gy, w, h];
                    if let Some(win) = &self.win {
                        win.set_outer_position(self.phys(self.target[0], self.target[1]));
                    }
                    return;
                }
                let at = proto::hit(self.at, self.cursor.0, self.cursor.1, w, h);
                if let Some(win) = &self.win {
                    win.set_cursor(match at {
                        Some(Hit::Edge) if self.at == Place::Bottom => CursorIcon::NsResize,
                        Some(Hit::Edge) => CursorIcon::EwResize,
                        Some(Hit::Close | Hit::Browser) => CursorIcon::Pointer,
                        Some(Hit::Head) => CursorIcon::Grab,
                        Some(Hit::History) if self.has_history => CursorIcon::Pointer,
                        Some(Hit::History) | None => CursorIcon::Default,
                    });
                }
                let edge = at == Some(Hit::Edge);
                if edge != self.hover {
                    self.hover = edge;
                    self.paint();
                }
            }
            WindowEvent::CursorLeft { .. } if self.hover => {
                self.hover = false;
                self.paint();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (w, h) = self.logical();
                if self.captcha && inside(hint_rects(w).1, self.cursor) {
                    return self.open_in_browser(el);
                }
                match proto::hit(self.at, self.cursor.0, self.cursor.1, w, h) {
                    Some(Hit::Close) => self.close(el),
                    Some(Hit::Browser) => self.open_in_browser(el),
                    Some(Hit::History) if self.has_history => {
                        if let Some(v) = &self.view {
                            let _ = v.load_url(&history_url());
                        }
                    }
                    Some(Hit::History) => {}
                    Some(Hit::Head) if self.dock.is_none() => {
                        // From where the card really is (a resize moved it).
                        if let Some(Ok(p)) = self.win.as_ref().map(|w| w.outer_position()) {
                            let (x, y) = (p.x - self.mon.0, p.y - self.mon.1);
                            self.target = [x as f64 / self.k, y as f64 / self.k, w, h];
                        }
                        self.grab = Some(self.cursor);
                    }
                    Some(Hit::Head) => {}
                    Some(Hit::Edge) => {
                        let double = (self.last_edge_press)
                            .is_some_and(|t| t.elapsed() < Duration::from_millis(400));
                        self.last_edge_press = Some(Instant::now());
                        if double {
                            self.reset();
                        } else if let Some(win) = &self.win {
                            self.resized = true;
                            self.size = Some((w, h)); // reported as the size at close
                            let _ = win.drag_resize_window(match self.at {
                                Place::Right => ResizeDirection::West,
                                Place::Left => ResizeDirection::East,
                                Place::Bottom => ResizeDirection::North,
                            });
                        }
                    }
                    None => {}
                }
            }
            // Dropped: it docks to the nearer side (animated), which is kept.
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } if self.grab.take().is_some() => {
                let from = self.target;
                let to = proto::dock(self.screen, from);
                let size = self.size.is_some().then_some((from[2], from[3]));
                (self.at, self.target) = proto::place(self.screen, to, size);
                if to != self.side {
                    self.side = to;
                    say(Line::Side(to));
                }
                self.dock = Some((Instant::now(), from));
                self.paint(); // the grip moves to the new inner edge
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                self.close(el)
            }
            _ => {}
        }
    }
}

/// Every navigation and new window the page asks for goes through `route`,
/// with WebView2's word on whether the user started it (`IsUserInitiated`).
/// New windows never open (the browser gets a clicked one).
fn guard_links(view: &WebView, t0: Instant) -> windows::core::Result<()> {
    use webview2_com::{
        NavigationStartingEventHandler, NewWindowRequestedEventHandler, take_pwstr,
    };
    use windows::core::{BOOL, PWSTR};

    let core = view.webview();
    let links = super::Links::default();
    let (links_nav, mut token) = (links.clone(), 0i64);
    let navigation = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let (mut uri, mut gesture) = (PWSTR::null(), BOOL::default());
        // SAFETY: COM calls on live event args; `uri` is taken (freed) once.
        unsafe {
            args.Uri(&mut uri)?;
            args.IsUserInitiated(&mut gesture)?;
            let url = take_pwstr(uri);
            if !super::route(t0, &url, false, gesture.as_bool(), &links_nav) {
                args.SetCancel(true)?;
            }
        }
        Ok(())
    }));
    let window = NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let (mut uri, mut gesture) = (PWSTR::null(), BOOL::default());
        // SAFETY: as above.
        unsafe {
            args.SetHandled(true)?; // never a WebView2 popup window
            args.Uri(&mut uri)?;
            args.IsUserInitiated(&mut gesture)?;
            super::route(t0, &take_pwstr(uri), true, gesture.as_bool(), &links);
        }
        Ok(())
    }));
    // SAFETY: registering handlers on our live web view.
    unsafe {
        core.add_NavigationStarting(&navigation, &mut token)?;
        core.add_NewWindowRequested(&window, &mut token)?;
    }
    Ok(())
}
