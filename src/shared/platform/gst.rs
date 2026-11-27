//! GStreamer from `libgstreamer-1.0.so.0`, loaded at run time with `dlopen` (Linux): nothing
//! is linked, a PC without it just has no ScreenCast stream and no recording. Two users: the
//! ScreenCast portal's frames (`capture::screencast`: pipewiresrc -> appsink) and the recorder's
//! encoder (`encode::gst`: appsrc -> encoder -> muxer -> file).
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::OnceLock;
use std::time::Duration;

use gtk::glib;

/// The few GStreamer calls we need.
pub(crate) struct Gst {
    parse_launch: unsafe extern "C" fn(*const c_char, *mut *mut glib::ffi::GError) -> *mut c_void,
    bin_get_by_name: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    set_state: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    sample_get_buffer: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    sample_get_caps: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    caps_get_structure: unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void,
    structure_get_int: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_int) -> c_int,
    buffer_map: unsafe extern "C" fn(*mut c_void, *mut MapInfo, c_int) -> c_int,
    buffer_unmap: unsafe extern "C" fn(*mut c_void, *mut MapInfo),
    buffer_new_allocate: unsafe extern "C" fn(*mut c_void, usize, *mut c_void) -> *mut c_void,
    element_get_bus: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    bus_timed_pop_filtered: unsafe extern "C" fn(*mut c_void, u64, c_int) -> *mut c_void,
    message_parse_error:
        unsafe extern "C" fn(*mut c_void, *mut *mut glib::ffi::GError, *mut *mut c_char),
    pub(crate) mini_object_unref: unsafe extern "C" fn(*mut c_void),
    object_unref: unsafe extern "C" fn(*mut c_void),
    factory_find: unsafe extern "C" fn(*const c_char) -> *mut c_void,
}

/// GstMapInfo (ABI-stable since GStreamer 1.0).
#[repr(C)]
struct MapInfo {
    memory: *mut c_void,
    flags: c_int,
    data: *mut u8,
    size: usize,
    maxsize: usize,
    user_data: [*mut c_void; 4],
    reserved: [*mut c_void; 4],
}

const STATE_NULL: c_int = 1;
const STATE_PLAYING: c_int = 4;
const MAP_READ: c_int = 1;
const MAP_WRITE: c_int = 2;
const MESSAGE_EOS: c_int = 1;
const MESSAGE_ERROR: c_int = 2;
// The two offsets below are those of a 64-bit build: another pointer width would put the fields
// elsewhere, so it does not build.
#[cfg(not(target_pointer_width = "64"))]
compile_error!("the GStreamer struct offsets of platform::gst are for 64-bit targets");
const _: () = assert!(std::mem::size_of::<usize>() == 8 && std::mem::size_of::<u64>() == 8);

/// Offsets in the public GstBuffer / GstMessage structs (64-bit): pts; the message type.
const BUFFER_PTS: usize = 72;
const MESSAGE_TYPE: usize = 64;

impl Gst {
    /// Loaded and initialised once; `None` without GStreamer.
    pub(crate) fn get() -> Option<&'static Gst> {
        static GST: OnceLock<Option<Gst>> = OnceLock::new();
        GST.get_or_init(Gst::load).as_ref()
    }

    /// Whether the element `name` is installed (a plugin may be missing).
    pub(crate) fn has(&self, name: &str) -> bool {
        let Ok(name) = CString::new(name) else {
            return false;
        };
        // SAFETY: a plain lookup; the factory it returns is ours to unref.
        unsafe {
            let factory = (self.factory_find)(name.as_ptr());
            if factory.is_null() {
                return false;
            }
            (self.object_unref)(factory);
        }
        true
    }

    // (each symbol's type is the field or binding it lands in)
    #[allow(clippy::missing_transmute_annotations)]
    fn load() -> Option<Gst> {
        // SAFETY: dlopen by name; the handle is kept open (never closed), so the
        // symbols stay valid for the process. Each is cast to its C signature.
        unsafe {
            let lib = libc::dlopen(c"libgstreamer-1.0.so.0".as_ptr(), libc::RTLD_NOW);
            if lib.is_null() {
                return None;
            }
            let sym = |name: &CStr| {
                let p = libc::dlsym(lib, name.as_ptr());
                (!p.is_null()).then_some(p)
            };
            macro_rules! f {
                ($name:literal) => {
                    std::mem::transmute::<*mut c_void, _>(sym($name)?)
                };
            }
            let init: unsafe extern "C" fn(*mut c_int, *mut c_void, *mut c_void) -> c_int =
                f!(c"gst_init_check");
            let gst = Gst {
                parse_launch: f!(c"gst_parse_launch"),
                bin_get_by_name: f!(c"gst_bin_get_by_name"),
                set_state: f!(c"gst_element_set_state"),
                sample_get_buffer: f!(c"gst_sample_get_buffer"),
                sample_get_caps: f!(c"gst_sample_get_caps"),
                caps_get_structure: f!(c"gst_caps_get_structure"),
                structure_get_int: f!(c"gst_structure_get_int"),
                buffer_map: f!(c"gst_buffer_map"),
                buffer_unmap: f!(c"gst_buffer_unmap"),
                buffer_new_allocate: f!(c"gst_buffer_new_allocate"),
                element_get_bus: f!(c"gst_element_get_bus"),
                bus_timed_pop_filtered: f!(c"gst_bus_timed_pop_filtered"),
                message_parse_error: f!(c"gst_message_parse_error"),
                mini_object_unref: f!(c"gst_mini_object_unref"),
                object_unref: f!(c"gst_object_unref"),
                factory_find: f!(c"gst_element_factory_find"),
            };
            if init(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == 0
            {
                return None;
            }
            Some(gst)
        }
    }

    /// `line` parsed (not started): the pipeline, or GStreamer's own words why not.
    fn parse(&'static self, line: &str) -> Result<*mut c_void, String> {
        let line = CString::new(line).map_err(|_| "GStreamer: a NUL in the pipeline")?;
        // SAFETY: plain GStreamer C API use; the pipeline of a failed parse is unreffed.
        unsafe {
            let mut error = std::ptr::null_mut();
            let pipe = (self.parse_launch)(line.as_ptr(), &mut error);
            if !error.is_null() {
                let why = CStr::from_ptr((*error).message)
                    .to_string_lossy()
                    .into_owned();
                glib::ffi::g_error_free(error);
                if !pipe.is_null() {
                    (self.object_unref)(pipe);
                }
                return Err(format!("GStreamer: {why}"));
            }
            match pipe.is_null() {
                true => Err("GStreamer: no pipeline".into()),
                false => Ok(pipe),
            }
        }
    }

    /// `line` parsed and playing, its appsink named `sink`; stopped and freed when the `Pipe` goes.
    pub(crate) fn launch(&'static self, line: &str) -> Result<Pipe, String> {
        let pipe = self.parse(line)?;
        // SAFETY: `pipe` is a live pipeline; what is taken is unreffed (`Pipe::drop`).
        unsafe {
            let sink = (self.bin_get_by_name)(pipe, c"sink".as_ptr());
            let pipe = Pipe {
                gst: self,
                pipe,
                sink,
            };
            if sink.is_null() {
                return Err("GStreamer: no sink".into());
            }
            (self.set_state)(pipe.pipe, STATE_PLAYING);
            Ok(pipe)
        }
    }

    /// `take` over the sample's mapped bytes and its width and height.
    /// SAFETY: `sample` is a live GstSample.
    pub(crate) unsafe fn read<T>(
        &self,
        sample: *mut c_void,
        take: impl FnOnce(&[u8], u32, u32) -> Result<T, String>,
    ) -> Result<T, String> {
        unsafe {
            let caps = (self.sample_get_caps)(sample);
            let buffer = (self.sample_get_buffer)(sample);
            if caps.is_null() || buffer.is_null() {
                return Err("empty frame".into());
            }
            let s = (self.caps_get_structure)(caps, 0);
            let (mut w, mut h) = (0, 0);
            (self.structure_get_int)(s, c"width".as_ptr(), &mut w);
            (self.structure_get_int)(s, c"height".as_ptr(), &mut h);
            let mut map: MapInfo = std::mem::zeroed();
            if (self.buffer_map)(buffer, &mut map, MAP_READ) == 0 {
                return Err("frame not readable".into());
            }
            let bytes = std::slice::from_raw_parts(map.data, map.size);
            let taken = take(bytes, w.max(0) as u32, h.max(0) as u32);
            (self.buffer_unmap)(buffer, &mut map);
            taken
        }
    }
}

/// A playing pipeline and its appsink; stopped and freed on drop.
pub(crate) struct Pipe {
    pub(crate) gst: &'static Gst,
    pipe: *mut c_void,
    sink: *mut c_void,
}

// SAFETY: GStreamer objects are thread-safe; a `Pipe` is used by one thread at a time.
unsafe impl Send for Pipe {}

impl Pipe {
    /// The sink's next sample within `wait` (null: none came; the caller unrefs it).
    pub(crate) fn pull(&self, wait: Duration) -> *mut c_void {
        let mut sample: *mut c_void = std::ptr::null_mut();
        // SAFETY: `sink` is a live appsink (`launch`).
        unsafe {
            glib::gobject_ffi::g_signal_emit_by_name(
                self.sink as *mut _,
                c"try-pull-sample".as_ptr(),
                wait.as_nanos() as u64,
                &mut sample as *mut *mut c_void,
            );
        }
        sample
    }

    /// The stream ended (the user stopped sharing in the desktop's menu).
    pub(crate) fn ended(&self) -> bool {
        let mut eos: glib::ffi::gboolean = 0;
        // SAFETY: a live appsink; "eos" is its boolean property, the list ends with NULL.
        unsafe {
            glib::gobject_ffi::g_object_get(
                self.sink as *mut _,
                c"eos".as_ptr(),
                &mut eos as *mut glib::ffi::gboolean,
                std::ptr::null::<c_char>(),
            );
        }
        eos != 0
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: ours from `launch`, freed once.
        unsafe {
            (self.gst.set_state)(self.pipe, STATE_NULL);
            if !self.sink.is_null() {
                (self.gst.object_unref)(self.sink);
            }
            (self.gst.object_unref)(self.pipe);
        }
    }
}

/// A pipeline whose `appsrc`s we feed (the recorder's encoder); set to NULL and freed on drop.
pub(crate) struct Feed {
    gst: &'static Gst,
    pipe: *mut c_void,
    /// The appsrc elements by name (an extra reference each).
    srcs: Vec<(&'static str, *mut c_void)>,
}

// SAFETY: as `Pipe`.
unsafe impl Send for Feed {}

impl Gst {
    /// `line` parsed and playing, its appsrcs `names` looked up for `Feed::push`.
    pub(crate) fn feed(&'static self, line: &str, names: &[&'static str]) -> Result<Feed, String> {
        let pipe = self.parse(line)?;
        let mut feed = Feed {
            gst: self,
            pipe,
            srcs: Vec::new(),
        };
        for name in names {
            let c = CString::new(*name).expect("no NUL");
            // SAFETY: a live pipeline; the element is unreffed in `Feed::drop`.
            let src = unsafe { (self.bin_get_by_name)(pipe, c.as_ptr()) };
            if src.is_null() {
                return Err(format!("GStreamer: no element {name}"));
            }
            feed.srcs.push((name, src));
        }
        // SAFETY: as above.
        unsafe { (self.set_state)(pipe, STATE_PLAYING) };
        Ok(feed)
    }
}

impl Feed {
    fn src(&self, name: &str) -> Option<*mut c_void> {
        self.srcs.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
    }

    /// One buffer of `bytes` into the appsrc `name`, stamped `pts` (the recording's own clock:
    /// pauses are not in it, so one timing model serves both OSes); waits while the pipeline
    /// is behind.
    pub(crate) fn push(&self, name: &str, bytes: &[u8], pts: Duration) -> Result<(), String> {
        let src = self.src(name).ok_or("no such source")?;
        let g = self.gst;
        // SAFETY: a fresh buffer we fill and unref (the signal takes its own reference);
        // `src` is a live appsrc.
        unsafe {
            let buf =
                (g.buffer_new_allocate)(std::ptr::null_mut(), bytes.len(), std::ptr::null_mut());
            if buf.is_null() {
                return Err("GStreamer: no memory for a buffer".into());
            }
            let mut map: MapInfo = std::mem::zeroed();
            if (g.buffer_map)(buf, &mut map, MAP_WRITE) == 0 {
                (g.mini_object_unref)(buf);
                return Err("GStreamer: a buffer could not be written".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), map.data, bytes.len());
            (g.buffer_unmap)(buf, &mut map);
            // GstBuffer's public layout (ABI-stable): the mini object (64 bytes on 64-bit), the
            // pool pointer, then pts at 72. Nothing else shares the buffer yet.
            (buf as *mut u8)
                .add(BUFFER_PTS)
                .cast::<u64>()
                .write(pts.as_nanos() as u64);
            let mut ret: c_int = 0;
            glib::gobject_ffi::g_signal_emit_by_name(
                src as *mut _,
                c"push-buffer".as_ptr(),
                buf,
                &mut ret as *mut c_int,
            );
            (g.mini_object_unref)(buf);
            match ret {
                r if r >= 0 => Ok(()),
                -3 => Err("the file was closed".into()),
                _ => Err(self
                    .error()
                    .unwrap_or_else(|| format!("GStreamer: flow {ret}"))),
            }
        }
    }

    /// The words of an error message (and the message freed).
    ///
    /// SAFETY: `msg` is a live GstMessage of type ERROR.
    unsafe fn error_words(&self, msg: *mut c_void) -> String {
        let g = self.gst;
        unsafe {
            let mut err = std::ptr::null_mut();
            let mut debug: *mut c_char = std::ptr::null_mut();
            (g.message_parse_error)(msg, &mut err, &mut debug);
            let why = match err.is_null() {
                true => "unknown".to_string(),
                false => CStr::from_ptr((*err).message)
                    .to_string_lossy()
                    .into_owned(),
            };
            if !err.is_null() {
                glib::ffi::g_error_free(err);
            }
            glib::ffi::g_free(debug as *mut c_void);
            (g.mini_object_unref)(msg);
            format!("GStreamer: {why}")
        }
    }

    /// The pipeline's first error message, if it posted one (a push that failed asks).
    pub(crate) fn error(&self) -> Option<String> {
        let g = self.gst;
        // SAFETY: the bus and message are ours, unreffed.
        unsafe {
            let bus = (g.element_get_bus)(self.pipe);
            if bus.is_null() {
                return None;
            }
            let msg = (g.bus_timed_pop_filtered)(bus, 0, MESSAGE_ERROR);
            (g.object_unref)(bus);
            (!msg.is_null()).then(|| self.error_words(msg))
        }
    }

    /// Ends every source and waits (at most `wait`) until the muxer wrote and closed the file.
    pub(crate) fn finish(&self, wait: Duration) -> Result<(), String> {
        let g = self.gst;
        for (_, src) in &self.srcs {
            let mut ret: c_int = 0;
            // SAFETY: a live appsrc; "end-of-stream" is its action signal.
            unsafe {
                glib::gobject_ffi::g_signal_emit_by_name(
                    *src as *mut _,
                    c"end-of-stream".as_ptr(),
                    &mut ret as *mut c_int,
                );
            }
        }
        let deadline = std::time::Instant::now() + wait;
        // SAFETY: the bus is ours, unreffed; a popped message is unreffed.
        unsafe {
            let bus = (g.element_get_bus)(self.pipe);
            if bus.is_null() {
                return Err("GStreamer: no bus".into());
            }
            let done = loop {
                // one pop for both: whichever comes first (an error is not read as the end)
                let msg = (g.bus_timed_pop_filtered)(bus, 100_000_000, MESSAGE_EOS | MESSAGE_ERROR);
                if !msg.is_null() {
                    let kind = (msg as *const u8).add(MESSAGE_TYPE).cast::<u32>().read();
                    break match kind as c_int & MESSAGE_ERROR != 0 {
                        true => Err(self.error_words(msg)),
                        false => {
                            (g.mini_object_unref)(msg);
                            Ok(())
                        }
                    };
                }
                if std::time::Instant::now() >= deadline {
                    break Err("GStreamer: the file did not close in time".into());
                }
            };
            (g.object_unref)(bus);
            done
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        // SAFETY: ours from `feed`, freed once.
        unsafe {
            (self.gst.set_state)(self.pipe, STATE_NULL);
            for (_, src) in &self.srcs {
                (self.gst.object_unref)(*src);
            }
            (self.gst.object_unref)(self.pipe);
        }
    }
}
