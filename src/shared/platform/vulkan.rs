//! The graphics cards Vulkan sees: kind, name, free and total video memory.
//! What the GPU translation helper (llama.cpp's Vulkan backend) would get,
//! asked BEFORE that helper is downloaded (live translation's GPU rule).
//!
//! The Vulkan loader is opened at run time (`vulkan-1.dll` / `libvulkan.so.1`,
//! shipped with every GPU driver): nothing is linked, and a PC without one
//! simply has no cards. Free memory = the driver's budget minus its usage
//! (`VK_EXT_memory_budget`, as llama.cpp reads it), else the heap's size.
//! Run it in a helper process (`ocr-worker`): opening the loader maps the
//! driver into the process that asks.
use std::ffi::{c_char, c_void};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gpu {
    /// A discrete card (else integrated: it shares the PC's memory).
    pub discrete: bool,
    pub name: String,
    /// Bytes of its largest device-local heap: free now, and in all.
    pub free: u64,
    pub total: u64,
}

/// Every integrated or discrete card (CPU and virtual devices left out);
/// none without a loader, a driver or on any Vulkan error.
pub fn gpus() -> Vec<Gpu> {
    #[cfg(windows)]
    const LOADER: &str = "vulkan-1.dll";
    #[cfg(not(windows))]
    const LOADER: &str = "libvulkan.so.1";
    // SAFETY: the system's Vulkan loader; its init runs nothing of ours.
    let Ok(lib) = (unsafe { libloading::Library::new(LOADER) }) else {
        return Vec::new();
    };
    // SAFETY: the loader's exported entry point, with its documented signature.
    match unsafe { lib.get::<GetProc>(b"vkGetInstanceProcAddr\0") } {
        // SAFETY: the calls follow the Vulkan 1.1 spec (structs below as the headers lay them out).
        Ok(get) => unsafe { list(*get) }.unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

type Handle = *mut c_void;
type Fn0 = unsafe extern "system" fn();
type GetProc = unsafe extern "system" fn(Handle, *const c_char) -> Option<Fn0>;
type CreateInstance =
    unsafe extern "system" fn(*const InstanceInfo, *const c_void, *mut Handle) -> i32;
type DestroyInstance = unsafe extern "system" fn(Handle, *const c_void);
type Enumerate = unsafe extern "system" fn(Handle, *mut u32, *mut Handle) -> i32;
type GetProps = unsafe extern "system" fn(Handle, *mut Props);
type GetMem = unsafe extern "system" fn(Handle, *mut MemProps);
type GetMem2 = unsafe extern "system" fn(Handle, *mut MemProps2);
type Extensions = unsafe extern "system" fn(Handle, *const c_char, *mut u32, *mut ExtProps) -> i32;

const API_1_1: u32 = (1 << 22) | (1 << 12);
const INTEGRATED: i32 = 1;
const DISCRETE: i32 = 2;
const DEVICE_LOCAL: u32 = 1;
const ST_APP_INFO: u32 = 0;
const ST_INSTANCE: u32 = 1;
const ST_MEM_PROPS2: u32 = 1_000_059_006;
const ST_BUDGET: u32 = 1_000_237_000;

#[repr(C)]
struct AppInfo {
    s_type: u32,
    next: *const c_void,
    app: *const c_char,
    app_version: u32,
    engine: *const c_char,
    engine_version: u32,
    api: u32,
}

#[repr(C)]
struct InstanceInfo {
    s_type: u32,
    next: *const c_void,
    flags: u32,
    app: *const AppInfo,
    layers: u32,
    layer_names: *const *const c_char,
    extensions: u32,
    extension_names: *const *const c_char,
}

/// VkPhysicalDeviceProperties up to the name; `rest` is room for the
/// fields after it (uuid, limits, sparse: ~550 bytes) that we never read.
#[repr(C)]
struct Props {
    api: u32,
    driver: u32,
    vendor: u32,
    device: u32,
    kind: i32,
    name: [c_char; 256],
    rest: [u64; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemType {
    flags: u32,
    heap: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemHeap {
    size: u64,
    flags: u32,
}

#[repr(C)]
struct MemProps {
    types: u32,
    type_list: [MemType; 32],
    heaps: u32,
    heap_list: [MemHeap; 16],
}

#[repr(C)]
struct MemProps2 {
    s_type: u32,
    next: *mut c_void,
    props: MemProps,
}

#[repr(C)]
struct Budget {
    s_type: u32,
    next: *mut c_void,
    budget: [u64; 16],
    usage: [u64; 16],
}

#[repr(C)]
struct ExtProps {
    name: [c_char; 256],
    version: u32,
}

fn zeroed<T>() -> T {
    // SAFETY: only used for the plain C structs above (integers, arrays, null pointers).
    unsafe { std::mem::zeroed() }
}

/// SAFETY: `get` is the loader's `vkGetInstanceProcAddr`.
unsafe fn list(get: GetProc) -> Option<Vec<Gpu>> {
    macro_rules! load {
        ($inst:expr, $name:literal, $ty:ty) => {
            // SAFETY: the name's function has the signature `$ty` (Vulkan spec).
            unsafe {
                std::mem::transmute::<Fn0, $ty>(get($inst, concat!($name, "\0").as_ptr().cast())?)
            }
        };
    }
    let create = load!(std::ptr::null_mut(), "vkCreateInstance", CreateInstance);
    let app = AppInfo {
        s_type: ST_APP_INFO,
        next: std::ptr::null(),
        app: c"OpenCircle".as_ptr(),
        app_version: 0,
        engine: std::ptr::null(),
        engine_version: 0,
        api: API_1_1,
    };
    let info = InstanceInfo {
        s_type: ST_INSTANCE,
        app: &app,
        ..zeroed()
    };
    let mut inst: Handle = std::ptr::null_mut();
    // SAFETY: valid create info; the instance is destroyed below.
    if unsafe { create(&info, std::ptr::null(), &mut inst) } != 0 || inst.is_null() {
        return None;
    }
    let destroy = load!(inst, "vkDestroyInstance", DestroyInstance);
    // SAFETY: `inst` is the live instance; destroyed once, after its last use.
    let out = unsafe { cards(get, inst) };
    unsafe { destroy(inst, std::ptr::null()) };
    out
}

/// SAFETY: `inst` is a live instance of `get`'s loader.
unsafe fn cards(get: GetProc, inst: Handle) -> Option<Vec<Gpu>> {
    macro_rules! load {
        ($name:literal, $ty:ty) => {
            // SAFETY: the name's function has the signature `$ty` (Vulkan spec).
            unsafe {
                std::mem::transmute::<Fn0, $ty>(get(inst, concat!($name, "\0").as_ptr().cast())?)
            }
        };
    }
    let enumerate = load!("vkEnumeratePhysicalDevices", Enumerate);
    let props_of = load!("vkGetPhysicalDeviceProperties", GetProps);
    let mem_of = load!("vkGetPhysicalDeviceMemoryProperties", GetMem);
    let mem2_of = load!("vkGetPhysicalDeviceMemoryProperties2", GetMem2);
    let exts_of = load!("vkEnumerateDeviceExtensionProperties", Extensions);
    let mut n = 0;
    // SAFETY: the count, then that many handles (a call may return fewer).
    unsafe {
        if enumerate(inst, &mut n, std::ptr::null_mut()) < 0 {
            return None;
        }
        let mut devs = vec![std::ptr::null_mut(); n as usize];
        if enumerate(inst, &mut n, devs.as_mut_ptr()) < 0 {
            return None;
        }
        devs.truncate(n as usize);
        let mut out = Vec::new();
        for dev in devs {
            let mut props: Props = zeroed();
            props_of(dev, &mut props);
            if props.kind != INTEGRATED && props.kind != DISCRETE {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(props.name.as_ptr());
            let budget = props.api >= API_1_1 && has_budget(exts_of, dev);
            let (mem, use_) = if budget {
                let mut b: Budget = zeroed();
                b.s_type = ST_BUDGET;
                let mut m: MemProps2 = zeroed();
                m.s_type = ST_MEM_PROPS2;
                m.next = (&mut b as *mut Budget).cast();
                mem2_of(dev, &mut m);
                (m.props, Some((b.budget, b.usage)))
            } else {
                let mut m: MemProps = zeroed();
                mem_of(dev, &mut m);
                (m, None)
            };
            let heaps = &mem.heap_list[..(mem.heaps as usize).min(16)];
            let Some((i, heap)) = (heaps.iter().enumerate())
                .filter(|(_, h)| h.flags & DEVICE_LOCAL != 0)
                .max_by_key(|(_, h)| h.size)
            else {
                continue;
            };
            let free = use_.map_or(heap.size, |(b, u)| b[i].saturating_sub(u[i]));
            out.push(Gpu {
                discrete: props.kind == DISCRETE,
                name: name.to_string_lossy().into_owned(),
                free: free.min(heap.size),
                total: heap.size,
            });
        }
        Some(out)
    }
}

/// SAFETY: `dev` is a live physical device; `exts_of` its instance's function.
unsafe fn has_budget(exts_of: Extensions, dev: Handle) -> bool {
    let mut n = 0;
    // SAFETY: the count, then that many entries.
    unsafe {
        if exts_of(dev, std::ptr::null(), &mut n, std::ptr::null_mut()) < 0 {
            return false;
        }
        let mut list: Vec<ExtProps> = (0..n).map(|_| zeroed()).collect();
        if exts_of(dev, std::ptr::null(), &mut n, list.as_mut_ptr()) < 0 {
            return false;
        }
        list.truncate(n as usize);
        list.iter().any(|e| {
            std::ffi::CStr::from_ptr(e.name.as_ptr()).to_bytes() == b"VK_EXT_memory_budget"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The structs are laid out as Vulkan's headers lay them out (x86_64).
    #[test]
    fn structs_match_the_vulkan_layouts() {
        use std::mem::{offset_of, size_of};
        assert_eq!(size_of::<MemProps>(), 520);
        assert_eq!(offset_of!(MemProps, heap_list), 264);
        assert_eq!(offset_of!(MemProps2, props), 16);
        assert_eq!(size_of::<Budget>(), 272);
        assert_eq!(size_of::<ExtProps>(), 260);
        assert_eq!(offset_of!(Props, kind), 16);
        assert_eq!(offset_of!(Props, name), 20);
        assert!(
            size_of::<Props>() >= 824,
            "room for the whole VkPhysicalDeviceProperties"
        );
        assert_eq!(offset_of!(InstanceInfo, app), 24);
        assert_eq!(offset_of!(AppInfo, api), 44);
    }

    /// No loader, no driver, or a software device only: no card, no crash
    /// (CI runners and this machine have no GPU).
    #[test]
    fn a_pc_without_a_gpu_lists_none_or_real_cards() {
        for g in gpus() {
            assert!(g.free <= g.total && !g.name.is_empty(), "{g:?}");
        }
    }
}
