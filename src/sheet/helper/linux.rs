//! Linux: a GTK window (X11 and Wayland alike) with WebKitGTK in the card.
//!
//! Wayland can't place a window, so the card lives in a work-area-sized
//! transparent window (maximized: GNOME paints a full-screen one black behind
//! its transparent parts); we place and size the card inside it, and a click
//! on the transparent rest closes it. GDK's work area is the whole monitor on
//! Wayland, so the card goes in what the window really got (its allocation).

use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;
use std::time::Instant;

use gtk::prelude::*;
use gtk::{cairo, gdk, glib};
use wry::{WebContext, WebViewBuilderExtUnix};

use super::proto::{self, Args, BODY_PAD, BODY_R, HEADER, Hit, Line, Place, RADIUS, Rect4};
use super::{
    CHIP, NO_RUNTIME, Page, builder, data_dir, history_url, icon, lerp, log, say, slid, slide_off,
};

/// A drag in progress: where it began (root), the card then, and whether it
/// moves the card (its header) or resizes it (its inner edge).
type Drag = ((f64, f64), Rect4, bool);

/// `history`: the History page (the user keeps one: its button shows).
pub fn run(args: &Args, page: Page, history: Option<String>, t0: Instant) -> u8 {
    if gtk::init().is_err() {
        eprintln!("[sheet] no display");
        return 1;
    }
    let Some(mon) =
        gdk::Display::default().and_then(|d| d.primary_monitor().or_else(|| d.monitor(0)))
    else {
        eprintln!("[sheet] no monitor");
        return 1;
    };
    let wa = mon.workarea();
    let (sw, sh) = (wa.width() as f64, wa.height() as f64);
    let kind = args.kind;
    let size = Rc::new(Cell::new(args.size));
    // The user's side; the card's place (a bottom card on narrow screens).
    let side = Rc::new(Cell::new(args.side));
    let (place0, target) = proto::place((sw, sh), side.get(), size.get());
    let at = Rc::new(Cell::new(place0));

    // The shared card's palette (the app's look), as CSS.
    let [bg, border, hover, text, muted] = args.colours;
    let css = gtk::CssProvider::new();
    let _ = css.load_from_data(
        format!(
            ".oc-card{{background:#{bg:06x};border:1px solid #{border:06x};\
             border-radius:{RADIUS}px;box-shadow:0 8px 24px rgba(0,0,0,{sa})}}\
             .oc-head{{border-bottom:1px solid #{border:06x}}}\
             .oc-icon{{color:#{text:06x};font-size:16px}}\
             .oc-title{{color:#{text:06x};font-size:16px}}\
             .oc-chip{{color:#{muted:06x};background:#{hover:06x};border-radius:999px;\
             padding:3px 8px;font-size:12px}}\
             .oc-x{{color:#{text:06x};background:none;border:none;box-shadow:none;\
             min-width:44px;min-height:44px;padding:0;font-size:15px}}\
             .oc-x:hover{{background:#{hover:06x};border-radius:12px}}\
             .oc-foot{{color:#{muted:06x};font-size:12px;border-top:1px solid #{border:06x};\
             padding:16px 20px}}\
             .oc-edge{{background:#{muted:06x};border-radius:2px}}\
             .oc-hint{{background:#{hover:06x};border-radius:12px;padding:6px 6px 6px 14px}}\
             .oc-hint label{{color:#{text:06x};font-size:13px}}\
             .oc-hint button{{color:#{text:06x};background:#{bg:06x};border:1px solid #{border:06x};\
             border-radius:999px;box-shadow:none;padding:4px 12px;font-size:13px}}",
            sa = args.shadow,
        )
        .as_bytes(),
    );
    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    if let Some(screen) = WidgetExt::screen(&win) {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        if let Some(v) = screen.rgba_visual() {
            win.set_visual(Some(&v));
        }
    }
    win.set_title("OpenCircle results");
    win.set_decorated(false);
    win.set_app_paintable(true);
    win.set_keep_above(true);
    win.set_skip_taskbar_hint(true);
    win.set_default_size(sw as i32, sh as i32);
    win.maximize();
    win.connect_draw(|_, cr| {
        cr.set_operator(cairo::Operator::Source);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();
        glib::Propagation::Proceed
    });

    let label = |text: &str, class: &str| {
        let l = gtk::Label::new(Some(text));
        l.style_context().add_class(class);
        l
    };
    let button = |text: &str, tip: &str| {
        let b = gtk::Button::with_label(text);
        b.style_context().add_class("oc-x");
        b.set_valign(gtk::Align::Center);
        b.set_tooltip_text(Some(tip));
        b
    };
    let outside = gtk::EventBox::new();
    outside.set_visible_window(false);
    let fixed = gtk::Fixed::new();
    let cardw = gtk::EventBox::new();
    cardw.set_visible_window(false);
    cardw.add_events(
        gdk::EventMask::BUTTON_PRESS_MASK
            | gdk::EventMask::BUTTON_RELEASE_MASK
            | gdk::EventMask::POINTER_MOTION_MASK
            | gdk::EventMask::LEAVE_NOTIFY_MASK,
    );
    let over = gtk::Overlay::new();
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.style_context().add_class("oc-card");
    // Header: icon, title, chip, Open in browser, × (as `card::frame`).
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    head.style_context().add_class("oc-head");
    head.set_size_request(-1, HEADER as i32);
    let first = label(icon(kind), "oc-icon");
    first.set_margin_start(20);
    head.pack_start(&first, false, false, 0);
    head.pack_start(&label(kind.title(), "oc-title"), false, false, 0);
    let chip = label(CHIP, "oc-chip");
    chip.set_valign(gtk::Align::Center);
    head.pack_start(&chip, false, false, 0);
    let close = button("✕", "Close");
    close.set_margin_end(12);
    close.connect_clicked(|_| gtk::main_quit());
    let to_browser = Rc::new(Cell::new(false));
    let open = button("↗", "Open in browser");
    open.connect_clicked({
        let to_browser = to_browser.clone();
        move |_| {
            to_browser.set(true);
            gtk::main_quit();
        }
    });
    head.pack_end(&close, false, false, 0);
    head.pack_end(&open, false, false, 0);
    // The web view, once built (History loads a page in it).
    let view: Rc<RefCell<Option<wry::WebView>>> = Rc::default();
    if history.is_some() {
        let past = button("↺", "History");
        past.connect_clicked({
            let view = view.clone();
            move |_| {
                if let Some(v) = view.borrow().as_ref() {
                    let _ = v.load_url(&history_url());
                }
            }
        });
        head.pack_end(&past, false, false, 0);
    }
    // Body: the page, padded like any card body, clipped to rounded corners.
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_margin_start(BODY_PAD as i32);
    content.set_margin_end(BODY_PAD as i32);
    content.set_margin_top(BODY_PAD as i32);
    content.set_margin_bottom(BODY_PAD as i32);
    content.connect_draw(|w, cr| {
        let (bw, bh, r) = (
            w.allocated_width() as f64,
            w.allocated_height() as f64,
            BODY_R,
        );
        cr.new_sub_path();
        cr.arc(bw - r, r, r, -PI / 2.0, 0.0);
        cr.arc(bw - r, bh - r, r, 0.0, PI / 2.0);
        cr.arc(r, bh - r, r, PI / 2.0, PI);
        cr.arc(r, r, r, PI, 1.5 * PI);
        cr.close_path();
        cr.clip();
        glib::Propagation::Proceed
    });
    let foot = label(kind.privacy(), "oc-foot");
    foot.set_ellipsize(gtk::pango::EllipsizeMode::End);
    foot.set_xalign(0.0);
    frame.pack_start(&head, false, false, 0);
    // Google's human check: a slim hint above the page (the user solves it
    // there, or takes the search to the browser; nothing here ever answers it).
    let hint = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hint.style_context().add_class("oc-hint");
    let words = gtk::Label::new(Some(
        "Google wants to check you're human. Solve it here, or open in your browser.",
    ));
    words.set_line_wrap(true);
    words.set_xalign(0.0);
    hint.pack_start(&words, true, true, 0);
    let in_browser = gtk::Button::with_label("Open in browser");
    in_browser.set_valign(gtk::Align::Center);
    in_browser.connect_clicked({
        let to_browser = to_browser.clone();
        move |_| {
            to_browser.set(true);
            gtk::main_quit();
        }
    });
    hint.pack_end(&in_browser, false, false, 0);
    hint.set_margin_start(BODY_PAD as i32);
    hint.set_margin_end(BODY_PAD as i32);
    hint.set_margin_top(12);
    // Its parts show now; the box itself only with the check (`no_show_all`
    // keeps the window's show_all off it, and its own show_all would skip them).
    words.show();
    in_browser.show();
    hint.set_no_show_all(true);
    frame.pack_start(&hint, false, false, 0);
    frame.pack_start(&content, true, true, 0);
    frame.pack_start(&foot, false, false, 0);
    // The resize grip on the inner edge, shown on hover.
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pill.style_context().add_class("oc-edge");
    let grip = {
        let pill = pill.clone();
        move |at: Place| {
            let (ph, pv) = if at == Place::Bottom {
                (40, 4)
            } else {
                (4, 40)
            };
            pill.set_size_request(ph, pv);
            let (ha, va) = match at {
                Place::Right => (gtk::Align::Start, gtk::Align::Center),
                Place::Left => (gtk::Align::End, gtk::Align::Center),
                Place::Bottom => (gtk::Align::Center, gtk::Align::Start),
            };
            pill.set_halign(ha);
            pill.set_valign(va);
        }
    };
    grip(at.get());
    pill.set_margin_start(2);
    pill.set_margin_end(2);
    pill.set_margin_top(2);
    pill.set_no_show_all(true);
    over.add(&frame);
    over.add_overlay(&pill);
    over.set_overlay_pass_through(&pill, true);
    cardw.add(&over);
    fixed.put(&cardw, 0, 0);
    outside.add(&fixed);
    win.add(&outside);

    // GtkFixed gives a child its natural size (the page's can be wider than the
    // card), so the card gets exactly its rect here, after Fixed's own layout.
    let rect = Rc::new(Cell::new(target));
    let shown = Rc::new(Cell::new(target));
    let area = Rc::new(Cell::new((sw, sh)));
    fixed.connect_size_allocate({
        let (cardw, shown, rect, area, size, side, at, grip) = (
            cardw.clone(),
            shown.clone(),
            rect.clone(),
            area.clone(),
            size.clone(),
            side.clone(),
            at.clone(),
            grip.clone(),
        );
        move |_, a| {
            let got = (a.width() as f64, a.height() as f64);
            if got != area.get() {
                area.set(got);
                let (p, t) = proto::place(got, side.get(), size.get());
                at.set(p);
                grip(p);
                rect.set(t);
                shown.set(t);
            }
            let r = shown.get();
            let (x, y) = (a.x() + r[0] as i32, a.y() + r[1] as i32);
            cardw.size_allocate(&gtk::Allocation::new(x, y, r[2] as i32, r[3] as i32));
        }
    });
    let apply = {
        let fixed = fixed.clone();
        move |r: Rect4| {
            shown.set(r);
            fixed.queue_resize();
        }
    };
    let (x0, y0) = slide_off(at.get(), 0.0);
    apply([target[0] + x0, target[1] + y0, target[2], target[3]]);
    cardw.set_opacity(0.0);

    outside.connect_button_press_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Stop
    });
    // The inner edge resizes (double-click: the default size); the header
    // moves the card, which docks to the nearer side on release.
    let resized = Rc::new(Cell::new(false));
    let drag: Rc<Cell<Option<Drag>>> = Rc::new(Cell::new(None));
    cardw.connect_button_press_event({
        let (drag, rect, apply, area, size, resized, side, at) = (
            drag.clone(),
            rect.clone(),
            apply.clone(),
            area.clone(),
            size.clone(),
            resized.clone(),
            side.clone(),
            at.clone(),
        );
        move |_, e| {
            let (x, y) = e.position();
            let r = rect.get();
            match proto::hit(at.get(), x, y, r[2], r[3]) {
                Some(Hit::Edge) if e.event_type() == gdk::EventType::DoubleButtonPress => {
                    size.set(None);
                    resized.set(true);
                    let d = proto::place(area.get(), side.get(), None).1;
                    rect.set(d);
                    apply(d);
                }
                Some(Hit::Edge) => drag.set(Some((e.root(), r, false))),
                Some(Hit::Head) if e.event_type() == gdk::EventType::ButtonPress => {
                    drag.set(Some((e.root(), r, true)));
                }
                _ => {}
            }
            glib::Propagation::Stop // never reaches `outside` (that closes)
        }
    });
    cardw.connect_motion_notify_event({
        let (drag, rect, apply, win, pill, area, at) = (
            drag.clone(),
            rect.clone(),
            apply.clone(),
            win.clone(),
            pill.clone(),
            area.clone(),
            at.clone(),
        );
        move |_, e| {
            let (rx, ry) = e.root();
            if let Some(((x0, y0), r0, moving)) = drag.get() {
                let r = if moving {
                    // The card follows the pointer (it docks on release).
                    [r0[0] + rx - x0, r0[1] + ry - y0, r0[2], r0[3]]
                } else {
                    proto::resized(at.get(), r0, rx - x0, ry - y0, area.get())
                };
                rect.set(r);
                apply(r);
                return glib::Propagation::Stop;
            }
            let (x, y) = e.position();
            let r = rect.get();
            let hit = proto::hit(at.get(), x, y, r[2], r[3]);
            pill.set_visible(hit == Some(Hit::Edge));
            let name = match (hit, at.get()) {
                (Some(Hit::Edge), Place::Bottom) => "ns-resize",
                (Some(Hit::Edge), _) => "ew-resize",
                (Some(Hit::Head), _) => "grab",
                _ => "default",
            };
            if let Some(w) = win.window() {
                w.set_cursor(gdk::Cursor::from_name(&w.display(), name).as_ref());
            }
            glib::Propagation::Stop
        }
    });
    cardw.connect_leave_notify_event({
        let pill = pill.clone();
        move |_, _| {
            pill.set_visible(false);
            glib::Propagation::Proceed
        }
    });
    cardw.connect_button_release_event({
        let (rect, size, resized, area, side, at) = (
            rect.clone(),
            size.clone(),
            resized.clone(),
            area.clone(),
            side.clone(),
            at.clone(),
        );
        let apply = apply.clone();
        move |_, _| {
            match drag.take() {
                // Dropped: it docks to the nearer side (animated), which is kept.
                Some((_, _, true)) => {
                    let from = rect.get();
                    let to = proto::dock(area.get(), from);
                    let (p, target) = proto::place(area.get(), to, size.get());
                    if to != side.get() {
                        side.set(to);
                        say(Line::Side(to));
                    }
                    at.set(p);
                    grip(p);
                    rect.set(target);
                    let (began, apply) = (Instant::now(), apply.clone());
                    glib::timeout_add_local(std::time::Duration::from_millis(8), move || {
                        let s = slid(began.elapsed());
                        apply(lerp(from, target, s));
                        if s >= 1.0 {
                            glib::ControlFlow::Break
                        } else {
                            glib::ControlFlow::Continue
                        }
                    });
                }
                Some(_) => {
                    let r = rect.get();
                    size.set(Some((r[2], r[3])));
                    resized.set(true);
                }
                None => {}
            }
            glib::Propagation::Stop
        }
    });
    win.connect_key_press_event(|_, e| {
        if e.keyval() == gdk::keys::constants::Escape {
            gtk::main_quit();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    win.connect_delete_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Proceed
    });

    let mut ctx = WebContext::new(Some(data_dir()));
    let captcha = move |on: bool| {
        if on != hint.is_visible() {
            hint.set_visible(on);
            log(
                t0,
                if on {
                    "human check shown"
                } else {
                    "human check gone"
                },
                "",
            );
        }
    };
    // ponytail: Linux shows the card at once (sliding in over the app's loading
    // card); wait for `drawing` as Windows does if that shows a blank body.
    let built = builder(
        &mut ctx,
        page,
        history,
        t0,
        gtk::main_quit,
        captcha,
        || {},
        bg,
    );
    match built.build_gtk(&content) {
        Ok(v) => {
            guard_links(&v, t0);
            *view.borrow_mut() = Some(v);
        }
        Err(e) => {
            eprintln!("[sheet] web view: {e}");
            return NO_RUNTIME;
        }
    }
    win.show_all();
    log(t0, "shown", "");
    say(Line::Ready);
    // Slide in and fade: from SLIDE_BY px out to its place.
    let began = Instant::now();
    glib::timeout_add_local(std::time::Duration::from_millis(8), {
        let (cardw, rect, at) = (cardw.clone(), rect.clone(), at.clone());
        move || {
            let s = slid(began.elapsed());
            let (dx, dy) = slide_off(at.get(), s);
            let t = rect.get();
            apply([t[0] + dx, t[1] + dy, t[2], t[3]]);
            cardw.set_opacity(s);
            if s >= 1.0 {
                log(t0, "slid in", "");
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        }
    });
    gtk::main();
    win.hide();
    if resized.get() {
        say(Line::Size(size.get().map(|s| proto::kept(at.get(), s))));
    }
    if to_browser.get()
        && let Some(Ok(url)) = view.borrow().as_ref().map(wry::WebView::url)
        && url.starts_with("https://")
    {
        say(Line::Leave(url));
    }
    0
}

/// Every navigation and new window the page asks for goes through `route`,
/// with WebKit's word on whether the user's own click or key started it.
fn guard_links(view: &wry::WebView, t0: Instant) {
    use webkit2gtk::{
        NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecisionExt,
        PolicyDecisionType, URIRequestExt, WebViewExt,
    };
    use wry::WebViewExtUnix;
    let links = super::Links::default();
    view.webview()
        .connect_decide_policy(move |_, decision, kind| {
            let new_window = match kind {
                PolicyDecisionType::NavigationAction => false,
                PolicyDecisionType::NewWindowAction => true,
                _ => return false, // responses: WebKit's default
            };
            let action = decision
                .downcast_ref::<NavigationPolicyDecision>()
                .and_then(|d| d.navigation_action());
            let Some(action) = action else {
                decision.ignore();
                return true;
            };
            let url = (action.request())
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                .unwrap_or_default();
            if super::route(t0, &url, new_window, action.is_user_gesture(), &links) {
                decision.use_();
            } else {
                decision.ignore();
            }
            true
        });
}
