//! The selection: overlay events, grabbing, moving, snapping, remembering.

use crate::ui::*;

impl App {
    pub(in crate::ui) fn on_overlay(&mut self, event: overlay::Event) -> Task<Message> {
        if let overlay::Event::Alt(alt) = event {
            self.o.alt = alt;
            return Task::none();
        }
        // A drag let go before the band under its press was read waits for
        // it, with the keys after it (Ctrl+C); a new press goes on without.
        if let Some(held) = &mut self.o.held {
            match event {
                overlay::Event::Press { .. } => {
                    let released = self.release_held();
                    return Task::batch([released, self.on_overlay(event)]);
                }
                overlay::Event::Hover(_) | overlay::Event::Move(_) => {}
                _ => {
                    held.push(event);
                    return Task::none();
                }
            }
        }
        // The editor, a toast or a scrolling capture has the screen.
        if self.o.edit.as_ref().is_some_and(Editing::is_open)
            || self.o.toast.is_some()
            || self.snip_pill()
        {
            return Task::none();
        }
        // Zoom and pan move the view; pointer events go on in image px.
        let event = match event {
            overlay::Event::Zoom { at, notches } => {
                if self.can_zoom() {
                    self.o.view = self.o.view.zoomed(at, notches);
                } else if let Purpose::Snip(snip) = &mut self.o.purpose {
                    snip.walk(notches); // control -> window -> screen, and back
                }
                return Task::none();
            }
            overlay::Event::PanStart(at) => {
                self.o.pan = self.can_zoom().then_some((at, self.o.view));
                return Task::none();
            }
            overlay::Event::PanMove(at) => {
                if let Some((from, start)) = self.o.pan {
                    self.o.view = start.panned(at - from);
                }
                return Task::none();
            }
            overlay::Event::ZoomReset => {
                self.o.view = self.o.view.reset();
                return Task::none();
            }
            overlay::Event::Press { at, shift } => {
                self.o.bar_focus = None; // a click puts the focus back on the overlay
                overlay::Event::Press {
                    at: self.o.view.to_image(at),
                    shift,
                }
            }
            overlay::Event::Move(at) => overlay::Event::Move(self.o.view.to_image(at)),
            overlay::Event::Hover(at) => overlay::Event::Hover(self.o.view.to_image(at)),
            event => event,
        };
        // Snip: the loupe follows the pointer; a drag's ends snap to edges.
        let event = match (&mut self.o.purpose, event) {
            (Purpose::Snip(snip), overlay::Event::Press { at, shift }) => {
                snip.hover(at);
                overlay::Event::Press {
                    at: snip.press(at),
                    shift,
                }
            }
            (Purpose::Snip(snip), overlay::Event::Move(at)) => {
                snip.hover(at);
                overlay::Event::Move(snip.snap(at))
            }
            (Purpose::Snip(snip), overlay::Event::Hover(at)) => {
                snip.hover(at);
                overlay::Event::Hover(at)
            }
            (_, event) => event,
        };
        // Over a translation: a tap opens a block's word card, E / A are the
        // card's; a new selection leaves the translation.
        if let Some((id, session)) = &mut self.o.translate {
            let id = *id;
            match event {
                overlay::Event::Key {
                    letter: 'e',
                    ctrl: false,
                } if session.has_card() => {
                    return self.on_translate(id, translate::Msg::Explain);
                }
                overlay::Event::Key {
                    letter: 'a',
                    ctrl: false,
                } if session.has_card() => {
                    return self.on_translate(id, translate::Msg::Ask);
                }
                overlay::Event::Key { .. } | overlay::Event::Enter | overlay::Event::Space => {
                    return Task::none();
                }
                overlay::Event::Release if overlay::select(&self.o.stroke, 1.0).is_none() => {
                    let at = self.o.stroke.last().copied().unwrap_or(Point::ORIGIN);
                    session.click(Point::new(at.x * self.o.scale, at.y * self.o.scale));
                    self.o.drawing = false;
                    self.o.stroke.clear();
                    return Task::none();
                }
                overlay::Event::Release => self.o.translate = None,
                _ => {}
            }
        }
        match event {
            // Select-text mode: a press on the box picks words, a drag stretches them.
            overlay::Event::Press {
                at: p,
                shift: false,
            } if self.picks_text_at(p) => {
                let (area, unit) = (self.area(), self.click(p));
                if let Some(mode) = &mut self.o.text_mode {
                    let lines = mode.lines.clone().unwrap_or_default();
                    mode.picked = word::Picked::new(lines, area, p, unit);
                    mode.picking = mode.picked.is_some();
                }
            }
            overlay::Event::Move(p) if self.o.text_mode.as_ref().is_some_and(|m| m.picking) => {
                if let Some(picked) = self.o.text_mode.as_mut().and_then(|m| m.picked.as_mut()) {
                    picked.extend(p);
                }
            }
            overlay::Event::Release if self.o.text_mode.as_ref().is_some_and(|m| m.picking) => {
                if let Some(mode) = &mut self.o.text_mode {
                    mode.picking = false;
                }
            }
            overlay::Event::Press { at: p, shift } => {
                self.o.pointer = p;
                // A new selection: the panel and the edits were about the old one.
                self.o.panel = None;
                self.o.text_mode = None;
                self.o.morph = None;
                self.o.edit = None;
                self.o.scan = None;
                self.taps = self.taps.wrapping_add(1); // a tap still reading is stale
                let unit = self.click(p);
                if !shift && let Some(grab) = self.grab_at(p) {
                    self.o.grab = Some(grab);
                    return Task::none();
                }
                // On text (once the screen is read): selected like in a document.
                // Shift+click stretches the text selection there.
                if shift
                    && self.text_at(p, unit).is_some()
                    && let Some(word) = &mut self.o.word
                    && word.extend(p)
                {
                    self.select_word();
                    self.o.text_press = Some(vec![p]);
                    return Task::none();
                }
                if !shift && let Some(word) = self.text_at(p, unit) {
                    self.o.word = Some(word);
                    self.o.text_used = true;
                    self.select_word();
                    self.o.text_press = Some(vec![p]);
                    return Task::none();
                }
                // Shift keeps the selection and adds the next one.
                self.o.adding = shift && matches!(self.o.purpose, Purpose::Search);
                match self.o.selection.take() {
                    Some(sel) if self.o.adding => {
                        self.o
                            .others
                            .push((sel, std::mem::take(&mut self.o.stroke)));
                    }
                    _ => self.o.others.clear(),
                }
                self.o.stroke = vec![p];
                self.o.drawing = true;
                // perhaps on text not read yet: its band now
                return self.read_more();
            }
            overlay::Event::Move(p) if self.o.text_press.is_some() => return self.drag_text(p),
            overlay::Event::Move(p) if self.o.grab.is_some() => self.drag_grab(p),
            overlay::Event::Move(p) if self.o.drawing => self.o.stroke.push(p),
            overlay::Event::Move(_) => {}
            overlay::Event::Hover(p) => {
                self.o.pointer = p;
                // Snip's Window mode: the window under the pointer lights up.
                if let Purpose::Snip(snip) = &self.o.purpose
                    && snip.kind == snip::Kind::Window
                {
                    match snip.window_at(p) {
                        Some(w) if self.o.selection.map(|s| s.area) != Some(w) => self.select(w),
                        Some(_) => {}
                        None => {
                            self.o.selection = None;
                            self.o.stroke.clear();
                        }
                    }
                }
            }
            overlay::Event::Release if self.o.text_press.take().is_some() => {
                if self.o.word.as_ref().is_some_and(word::Picked::is_empty) {
                    self.o.word = None; // a click on text: nothing selected
                }
                return self.selected();
            }
            overlay::Event::Release if let Some(grab) = self.o.grab.take() => {
                return if grab.moved {
                    // The search first: a waiting quick action (`selected`)
                    // turns the purpose back to Search, and must not get it too.
                    let search = self.auto_search();
                    Task::batch([self.selected(), search])
                } else {
                    self.tap(grab.from) // a click on the selection
                };
            }
            // A drag that may have begun on text not read yet: the band under
            // its press decides (text or a stroke) once read (`release_held`).
            overlay::Event::Release
                if !self.o.adding
                    && let Some(&start) = self.o.stroke.first()
                    && self.o.reading.is_some_and(|band| band.contains(start))
                    && !self.o.screen_text.knows(start.y)
                    && !overlay::is_tap(&self.o.stroke, self.o.view.zoom()) =>
            {
                self.o.held = Some(Vec::new());
            }
            overlay::Event::Release => {
                self.o.drawing = false;
                let mut settle = Task::none();
                let zoom = self.o.view.zoom();
                let mut settled = overlay::settle(&self.o.stroke, zoom, self.o.screen);
                // Snap: the stroke becomes a rounded box (it morphs there).
                if let Some((sel, kept)) = &mut settled
                    && self.snaps()
                {
                    if sel.kind != overlay::Kind::Drag {
                        self.o.morph = Some((kept.clone(), Instant::now()));
                        settle = background(|tx| {
                            std::thread::sleep(overlay::MORPH + Duration::from_millis(20));
                            let _ = tx.unbounded_send(());
                        })
                        .map(|()| Message::Settled);
                    }
                    (*sel, *kept) = overlay::snap(sel.area);
                }
                self.o.selection = settled.as_ref().map(|(sel, _)| *sel);
                let drawn = match settled {
                    Some((_, kept)) => std::mem::replace(&mut self.o.stroke, kept),
                    None => self.o.stroke.clone(),
                };
                if let Purpose::Snip(snip) = &self.o.purpose {
                    let area = self.o.selection.map(|s| s.area);
                    let at = drawn.last().copied().unwrap_or(Point::ORIGIN);
                    let next = snip.pick(area, at, self.o.screen);
                    self.remember_region();
                    return self.snip_next(next);
                }
                if std::mem::take(&mut self.o.adding)
                    && self.o.selection.is_none()
                    && let Some((sel, stroke)) = self.o.others.pop()
                {
                    // Nothing added (a tap, a line): the last one is current again.
                    (self.o.selection, self.o.stroke) = (Some(sel), stroke);
                    // Shift+click on text with words picked: stretch them there.
                    if sel.kind == overlay::Kind::Word
                        && self.o.word.is_some()
                        && overlay::is_tap(&drawn, zoom)
                    {
                        return self.extend_words(drawn[0]);
                    }
                    return Task::none();
                }
                if self.o.selection.is_some() {
                    self.o.word = None;
                } else if overlay::is_tap(&drawn, zoom) {
                    return self.tap(drawn[0]);
                }
                // The search first: a waiting quick action (`selected`) turns
                // the purpose back to Search, and must not get it too.
                let search = match self.o.others.is_empty() {
                    true => self.auto_search(),
                    false => Task::none(), // Shift: more to come
                };
                return Task::batch([self.selected(), search, settle]);
            }
            // Tab moved the focus onto a bar button: Enter / Space press it.
            overlay::Event::Enter | overlay::Event::Space
                if let Some(stop) = self.o.bar_focus
                    && self.bottom_bar_shown() =>
            {
                return self.activate(stop);
            }
            overlay::Event::Tab { back } if self.bottom_bar_shown() => return self.tab_bar(back),
            overlay::Event::Enter if matches!(self.o.purpose, Purpose::Search) => {
                return self.run(actions::default_for(&config::current()));
            }
            // Ctrl+A: all the text on the screen.
            overlay::Event::Key {
                letter: 'a',
                ctrl: true,
            } if self.reads_text() => {
                // all of it once read (the rest is read now, nearest the pointer first)
                self.o.want_all = true;
                self.o.text_used = true;
                let done = self.more_text();
                return Task::batch([done, self.read_more()]);
            }
            overlay::Event::Key { letter, ctrl } if matches!(self.o.purpose, Purpose::Search) => {
                // Nothing selected: Ask AI, Translate and Edit take the whole screen
                // (as the bottom bar's buttons do).
                let bar = match actions::for_key(&self.o.labs, letter, ctrl).map(|a| a.id) {
                    Some(id) if id == actions::ask::ACTION.id => Some(Message::AskButton),
                    Some(id) if id == actions::translate::ACTION.id => {
                        Some(Message::TranslateScreen)
                    }
                    Some(id) if id == actions::edit::ACTION.id => Some(Message::EditScreen),
                    _ => None,
                };
                if let (None, Some(message)) = (self.o.selection, bar) {
                    return self.update(message);
                }
                if let Some(action) = actions::for_key(&self.o.labs, letter, ctrl) {
                    return self.run(action);
                }
            }
            // Snip: Ctrl+C copies what a click would capture here (the picked area,
            // else the window or control outlined under the pointer), whatever
            // `snip.after` says.
            overlay::Event::Key {
                letter: 'c',
                ctrl: true,
            } if matches!(self.o.purpose, Purpose::Snip(_)) => {
                let Purpose::Snip(snip) = &self.o.purpose else {
                    return Task::none();
                };
                let at = snip.pointer.unwrap_or(Point::ORIGIN);
                let area = self.o.selection.map(|s| s.area);
                let Some(snip::Next::Edit(area)) = snip.pick(area, at, self.o.screen) else {
                    return Task::none();
                };
                self.select(area);
                let Some(crop) = self.crop() else {
                    return Task::none();
                };
                return self.snip_routed(crop, true, (true, false));
            }
            overlay::Event::Arrow {
                dx,
                dy,
                ctrl,
                shift,
            } => self.nudge(dx, dy, ctrl, shift),
            overlay::Event::Enter
            | overlay::Event::Space
            | overlay::Event::Tab { .. }
            | overlay::Event::Alt(_)
            | overlay::Event::Key { .. } => {}
            // mapped above
            overlay::Event::Zoom { .. }
            | overlay::Event::PanStart(_)
            | overlay::Event::PanMove(_)
            | overlay::Event::ZoomReset => {}
        }
        Task::none()
    }

    /// The frame can be zoomed: Circle to Search and quick actions (Snip, the
    /// editor and a translation draw at 1x).
    pub(in crate::ui) fn can_zoom(&self) -> bool {
        matches!(self.o.purpose, Purpose::Search | Purpose::Quick(..))
            && self.o.edit.is_none()
            && self.o.translate.is_none()
    }

    /// The selection shows its handles (and can be resized / moved) while the
    /// action bar is there.
    pub(in crate::ui) fn handles(&self) -> bool {
        let snapped = self
            .o
            .selection
            .is_some_and(|s| s.kind == overlay::Kind::Snap);
        matches!(self.o.purpose, Purpose::Search)
            && !self.o.drawing
            && (self.o.panel.is_none() || snapped)
            && self.o.edit.is_none()
            && self.o.translate.is_none()
    }

    /// A press at `p` (image px) on the selection: a corner or an edge resizes
    /// it; inside moves it, unless the press is on text (that selects text).
    pub(in crate::ui) fn grab_at(&self, p: Point) -> Option<Grab> {
        // Selected text is not a box: a press on it starts a new selection.
        let sel = (self.o.selection).filter(|s| self.handles() && s.kind != overlay::Kind::Word)?;
        let on_screen = self.o.view.rect_to_screen(sel.area);
        let grip = overlay::grip_at(sel.kind, on_screen, self.o.view.to_screen(p));
        let inside = sel.area.contains(p)
            && (self.o.alt
                || self.o.text_mode.is_none() && self.text_at(p, word::Unit::Caret).is_none());
        (grip.is_some() || inside).then(|| Grab {
            grip,
            from: p,
            start: (sel, self.o.stroke.clone()),
            moved: false,
        })
    }

    /// Resizes / moves the grabbed selection to the pointer at `p` (image px),
    /// never off the screen. A freehand loop scales with its box; anything
    /// else becomes that box.
    pub(in crate::ui) fn drag_grab(&mut self, p: Point) {
        let Some(grab) = &mut self.o.grab else { return };
        let zoom = self.o.view.zoom();
        grab.moved |= !overlay::is_tap(&[grab.from, p], zoom);
        if !grab.moved {
            return;
        }
        let (start, stroke) = &grab.start;
        let min = overlay::MIN_SIDE / zoom;
        let delta = p - grab.from;
        let on = |to: Point| {
            Point::new(
                to.x.clamp(0.0, self.o.screen.width),
                to.y.clamp(0.0, self.o.screen.height),
            )
        };
        let area = match grab.grip {
            // the corner follows the pointer from where it was grabbed (a bracket arm is off it)
            Some(overlay::Grip::Corner(c)) => overlay::resize(
                start.area,
                c,
                on(overlay::corner(start.area, c) + delta),
                min,
            ),
            // (an edge's coordinate is its first corner's)
            Some(overlay::Grip::Edge(e)) => overlay::resize_edge(
                start.area,
                e,
                on(overlay::corner(start.area, e) + delta),
                min,
            ),
            None => overlay::keep_inside(start.area + delta, self.o.screen),
        };
        let kind = match start.kind {
            overlay::Kind::Loop | overlay::Kind::Scribble | overlay::Kind::Snap => start.kind,
            overlay::Kind::Drag | overlay::Kind::Word => overlay::Kind::Drag,
        };
        self.o.morph = None;
        self.o.stroke = overlay::refit(stroke, start.area, area);
        self.o.selection = Some(overlay::Selection { area, kind });
        self.o.word = None; // the words are no longer what is selected
    }

    /// An arrow key on the box (`overlay::Event::Arrow`): 1 px (Shift: 10) on
    /// the overlay, kept on the screen and over the minimum size.
    pub(in crate::ui) fn nudge(&mut self, dx: i8, dy: i8, resize: bool, big: bool) {
        let Some(sel) = self
            .o
            .selection
            .filter(|s| self.handles() && s.kind != overlay::Kind::Word)
        else {
            return;
        };
        let one = self.o.view.to_image(Point::new(1.0, 1.0)) - self.o.view.to_image(Point::ORIGIN);
        let by = Vector::new(dx as f32 * one.x, dy as f32 * one.y) * if big { 10.0 } else { 1.0 };
        let min = overlay::MIN_SIDE / self.o.view.zoom();
        let a = sel.area;
        let area = if resize {
            let fit = |len: f32, at: f32, max: f32| (len).min(max - at).max(min);
            Rectangle::new(
                a.position(),
                Size::new(
                    fit(a.width + by.x, a.x, self.o.screen.width),
                    fit(a.height + by.y, a.y, self.o.screen.height),
                ),
            )
        } else {
            overlay::keep_inside(a + by, self.o.screen)
        };
        self.o.morph = None;
        self.o.panel = None;
        self.o.text_mode = None;
        self.o.scan = None;
        self.o.stroke = overlay::refit(&self.o.stroke, a, area);
        self.o.selection = Some(overlay::Selection { area, ..sel });
        self.o.word = None;
        self.remember_region();
    }

    /// The selection on the shown monitor, for `last_region`.
    pub(in crate::ui) fn remember_region(&mut self) {
        if let Some(sel) = self.o.selection {
            self.regions.retain(|(origin, _)| *origin != self.o.origin);
            self.regions.push((self.o.origin, sel.area));
        }
    }

    /// A selection was made (drawn, moved, a word tapped): remembered, and a
    /// quick action waiting for it runs. No selection: the stroke goes.
    pub(in crate::ui) fn selected(&mut self) -> Task<Message> {
        if self.o.selection.is_none() {
            self.o.stroke.clear();
            return Task::none();
        }
        self.remember_region();
        match (&self.o.purpose, self.o.selection) {
            (Purpose::Quick(quick, _), _) => {
                let quick = *quick;
                self.run_quick(quick)
            }
            // A tapped word is text OCR just read: no QR code, not protected.
            (Purpose::Search, Some(sel)) if sel.kind != overlay::Kind::Word => self.scan(sel.area),
            _ => Task::none(),
        }
    }

    /// Makes `area` the selection, drawn as a dragged rectangle.
    pub(in crate::ui) fn select(&mut self, area: Rectangle) {
        self.o.selection = Some(overlay::Selection {
            area,
            kind: overlay::Kind::Drag,
        });
        self.o.stroke = vec![area.position(), area.position() + Vector::from(area.size())];
    }

    /// A finished stroke snaps to a box (`[selection] style`; not in Snip).
    pub(in crate::ui) fn snaps(&self) -> bool {
        self.o.behaviour.style == SelectStyle::Snap
            && matches!(self.o.purpose, Purpose::Search | Purpose::Quick(..))
    }

    /// The selection's box (image px), or nothing.
    pub(in crate::ui) fn area(&self) -> Rectangle {
        self.o.selection.map(|s| s.area).unwrap_or_default()
    }

    /// The previous selection on the shown monitor.
    pub(in crate::ui) fn last_region(&self) -> Option<Rectangle> {
        let mine = self
            .regions
            .iter()
            .find(|(origin, _)| *origin == self.o.origin);
        mine.map(|(_, area)| *area)
    }
}
