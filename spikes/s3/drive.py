"""Spike S3 helper (X11/XWayland): draw a circle on the open overlay with XTest,
then press Esc.  Usage: python3 drive.py
The PNG comes from the app itself (OPENCIRCLE_TEST_SHOT): WSLg's rootless XWayland
refuses GetImage on the root and on the overlay window (BadMatch)."""
import ctypes as C, math, sys, time

x = C.cdll.LoadLibrary("libX11.so.6")
xt = C.cdll.LoadLibrary("libXtst.so.6")
x.XOpenDisplay.restype = C.c_void_p
x.XDefaultRootWindow.restype = C.c_ulong
x.XGetImage.restype = C.c_void_p
d = x.XOpenDisplay(None)
root = x.XDefaultRootWindow(C.c_void_p(d))


class Attrs(C.Structure):
    _fields_ = [("x", C.c_int), ("y", C.c_int), ("width", C.c_int), ("height", C.c_int),
                ("border_width", C.c_int), ("depth", C.c_int), ("visual", C.c_void_p),
                ("root", C.c_ulong), ("class_", C.c_int), ("bit_gravity", C.c_int),
                ("win_gravity", C.c_int), ("backing_store", C.c_int),
                ("backing_planes", C.c_ulong), ("backing_pixel", C.c_ulong),
                ("save_under", C.c_int), ("colormap", C.c_ulong), ("map_installed", C.c_int),
                ("map_state", C.c_int), ("all_event_masks", C.c_long),
                ("your_event_mask", C.c_long), ("do_not_propagate_mask", C.c_long),
                ("override_redirect", C.c_int), ("screen", C.c_void_p)]


def overlay_window(w=None, best=None):
    """Largest viewable window anywhere in the tree (the full-screen overlay)."""
    r, p, kids, n = C.c_ulong(), C.c_ulong(), C.POINTER(C.c_ulong)(), C.c_uint()
    x.XQueryTree(C.c_void_p(d), C.c_ulong(w or root), C.byref(r), C.byref(p), C.byref(kids), C.byref(n))
    for i in range(n.value):
        a = Attrs()
        x.XGetWindowAttributes(C.c_void_p(d), C.c_ulong(kids[i]), C.byref(a))
        if a.map_state == 2 and (best is None or a.width * a.height >= best[1].width * best[1].height):
            best = (kids[i], a)
        best = overlay_window(kids[i], best)
    return best


def move(px, py):
    xt.XTestFakeMotionEvent(C.c_void_p(d), -1, int(px), int(py), 0)
    x.XFlush(C.c_void_p(d))
    time.sleep(0.008)


win, a = overlay_window()
cx, cy, rx, ry = a.x + 460, a.y + 420, 200, 190
for k in range(10):
    move(cx + rx - 40 + 4 * k, cy + 20 - 2 * k)
time.sleep(0.2)
move(cx + rx, cy)
time.sleep(0.2)
xt.XTestFakeButtonEvent(C.c_void_p(d), 1, 1, 0)
for i in range(0, 75):  # a little over one turn, slightly wobbly like a hand
    t = i / 64 * 2 * math.pi
    w = 1 + 0.04 * math.sin(3 * t)
    move(cx + rx * w * math.cos(t), cy + ry * w * math.sin(t))
xt.XTestFakeButtonEvent(C.c_void_p(d), 1, 0, 0)
x.XFlush(C.c_void_p(d))
time.sleep(0.5)  # the app saves OPENCIRCLE_TEST_SHOT on release

# Close the overlay again (Esc) so the run doesn't leave the screen covered.
esc = x.XKeysymToKeycode(C.c_void_p(d), C.c_ulong(0xFF1B))
xt.XTestFakeKeyEvent(C.c_void_p(d), esc, 1, 0)
xt.XTestFakeKeyEvent(C.c_void_p(d), esc, 0, 0)
x.XFlush(C.c_void_p(d))
