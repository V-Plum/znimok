# -*- coding: utf-8 -*-
"""Тестове вікно для перевірки захоплення вікна (ZK-15, P2).

  python testwin.py [секунд] [x y w h]

Вікно з рамкою й заголовком «ZK15 test window»: клієнтська область синя (0,0,255), угорі червона смуга
40 px (255,0,0), у лівому верхньому куті клієнтської області зелений квадрат 32×32 (0,255,0).
Процес Per-Monitor-v2 — координати фізичні, як у znimok-p2.
"""
import ctypes, ctypes.wintypes as W, sys, time

u32 = ctypes.windll.user32
g32 = ctypes.windll.gdi32
u32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
secs = float(sys.argv[1]) if len(sys.argv) > 1 else 30
X, Y, WW, WH = (int(v) for v in sys.argv[2:6]) if len(sys.argv) >= 6 else (300, 200, 800, 500)

WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, W.HWND, ctypes.c_uint, W.WPARAM, W.LPARAM)
u32.DefWindowProcW.restype = ctypes.c_ssize_t
u32.DefWindowProcW.argtypes = [W.HWND, ctypes.c_uint, W.WPARAM, W.LPARAM]
u32.CreateWindowExW.restype = W.HWND
u32.CreateWindowExW.argtypes = [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                W.HWND, W.HMENU, W.HINSTANCE, W.LPVOID]


class WNDCLASSW(ctypes.Structure):
    _fields_ = [("style", ctypes.c_uint), ("lpfnWndProc", WNDPROC), ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
                ("hInstance", W.HINSTANCE), ("hIcon", W.HICON), ("hCursor", W.HANDLE), ("hbrBackground", W.HBRUSH),
                ("lpszMenuName", W.LPCWSTR), ("lpszClassName", W.LPCWSTR)]


class PAINTSTRUCT(ctypes.Structure):
    _fields_ = [("hdc", W.HDC), ("fErase", W.BOOL), ("rcPaint", W.RECT), ("fRestore", W.BOOL), ("fIncUpdate", W.BOOL),
                ("rgbReserved", ctypes.c_byte * 32)]


def box(dc, l, t, r, b, colorref):
    rc = W.RECT(l, t, r, b)
    br = g32.CreateSolidBrush(colorref)
    u32.FillRect(dc, ctypes.byref(rc), br)
    g32.DeleteObject(br)


@WNDPROC
def proc(h, m, wp, lp):
    if m == 0x000F:  # WM_PAINT
        ps = PAINTSTRUCT()
        dc = u32.BeginPaint(h, ctypes.byref(ps))
        r = W.RECT()
        u32.GetClientRect(h, ctypes.byref(r))
        box(dc, 0, 0, r.right, r.bottom, 0xFF0000)      # синій (COLORREF 0xBBGGRR)
        box(dc, 0, 0, r.right, 40, 0x0000FF)            # червона смуга
        box(dc, 0, 0, 32, 32, 0x00FF00)                 # зелений квадрат
        u32.EndPaint(h, ctypes.byref(ps))
        return 0
    if m == 0x0002:  # WM_DESTROY
        u32.PostQuitMessage(0)
        return 0
    return u32.DefWindowProcW(h, m, wp, lp)


hi = ctypes.windll.kernel32.GetModuleHandleW(None)
wc = WNDCLASSW()
wc.lpfnWndProc = proc
wc.hInstance = hi
wc.lpszClassName = "ZK15TestWindow"
wc.hCursor = u32.LoadCursorW(None, ctypes.c_void_p(32512))
u32.RegisterClassW(ctypes.byref(wc))
WS_OVERLAPPEDWINDOW, WS_VISIBLE = 0x00CF0000, 0x10000000
h = u32.CreateWindowExW(0, "ZK15TestWindow", "ZK15 test window", WS_OVERLAPPEDWINDOW | WS_VISIBLE, X, Y, WW, WH, None, None, hi, None)
u32.SetForegroundWindow(h)
print(f"HWND 0x{h:X}", flush=True)
end = time.time() + secs
msg = W.MSG()
while time.time() < end:
    while u32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
        u32.TranslateMessage(ctypes.byref(msg))
        u32.DispatchMessageW(ctypes.byref(msg))
    time.sleep(0.01)
u32.DestroyWindow(h)
