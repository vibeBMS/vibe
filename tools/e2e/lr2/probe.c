/* lr2probe: checks, on a CI runner, what Lunatic Rave 2 needs from Windows,
 * without LR2: a Direct3D 9 device, a sound device, the code page, and keys
 * sent by SendInput at set times. Nothing here comes from LR2; it only uses
 * the Windows APIs a DirectX 9 game uses.
 *
 *   lr2probe d3d9 SECONDS [fullscreen]   a D3D9 device, drawn to for SECONDS
 *   lr2probe sound                       waveOut, DirectSound, WASAPI endpoints
 *   lr2probe acp [OUT]                   code page, locale, Japanese fonts
 *   lr2probe recv N CSV [d3d9]           a window that logs N key presses (with
 *                                        d3d9: drawing with D3D9, one poll a frame)
 *   lr2probe send N PERIOD_MS CSV        N presses of Z, PERIOD_MS apart, to it
 *   lr2probe focus TITLE                 brings a window to the front
 *   lr2probe windows                     lists the visible top-level windows
 *   lr2probe drop TITLE X Y PATH         drags PATH (a file or folder) with the
 *                                        mouse onto X, Y in the window
 *   lr2probe ui TITLE ACTION ...         drives a dialog through its controls:
 *       dump                  every control: id, class, text, check, shown
 *       click [p/]ID          presses a button (p/: on the shown tab page)
 *       check [p/]ID 0|1      sets a check box or radio button
 *       text [p/]ID TEXT      sets a control's text
 *       items [p/]ID          lists a combo box's items
 *       select [p/]ID TEXT    selects a combo box's item, as a person's
 *                             choice would (CBN_SELCHANGE to its dialog)
 *       mouse X Y             clicks at X, Y in the window
 *       key VK                presses a virtual key (hex) in the window
 *
 * Times in the CSV files are QueryPerformanceCounter in microseconds, which
 * all processes share. Built 32-bit, as LR2 is (build.cmd).
 */
#define WIN32_LEAN_AND_MEAN
#define DIRECTINPUT_VERSION 0x0800
#define INITGUID
#include <windows.h>
#include <mmsystem.h>
#include <d3d9.h>
#include <dsound.h>
#include <dinput.h>
#include <objbase.h>
#include <shellapi.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static LARGE_INTEGER qpf;

static long long now_us(void)
{
    LARGE_INTEGER c;
    QueryPerformanceCounter(&c);
    return (long long)(c.QuadPart * 1000000.0 / qpf.QuadPart);
}

/* ---------------------------------------------------------------- window */

#define RECV_TITLE "lr2probe-recv"
#define SCAN_Z 0x2c
#define MAX_EVENTS 4096

typedef struct {
    char src;          /* 'm' window message, 'd' DirectInput */
    long long t_us;    /* when this process saw it */
    unsigned code;     /* scan code */
    int down;
    unsigned stamp_ms; /* the message's or DirectInput's own time stamp */
} Event;

static Event events[MAX_EVENTS];
static int n_events;

static void record(char src, unsigned code, int down, unsigned stamp)
{
    if (n_events < MAX_EVENTS) {
        Event *e = &events[n_events++];
        e->src = src;
        e->t_us = now_us();
        e->code = code;
        e->down = down;
        e->stamp_ms = stamp;
    }
}

static LRESULT CALLBACK wndproc(HWND h, UINT m, WPARAM w, LPARAM l)
{
    switch (m) {
    case WM_KEYDOWN:
    case WM_KEYUP:
        /* a repeat (bit 30 set on a down) is not a new press */
        if (m == WM_KEYDOWN && (l & (1 << 30)))
            return 0;
        record('m', (unsigned)((l >> 16) & 0xff), m == WM_KEYDOWN, (unsigned)GetMessageTime());
        return 0;
    case WM_CLOSE:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcA(h, m, w, l);
}

static HWND make_window(const char *title, int w, int h, DWORD style)
{
    WNDCLASSA wc;
    RECT r = {0, 0, w, h};
    HWND hwnd;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = wndproc;
    wc.hInstance = GetModuleHandleA(NULL);
    wc.lpszClassName = "lr2probe";
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)GetStockObject(BLACK_BRUSH);
    RegisterClassA(&wc);
    AdjustWindowRect(&r, style, FALSE);
    hwnd = CreateWindowExA(0, "lr2probe", title, style | WS_VISIBLE, 0, 0, r.right - r.left,
                           r.bottom - r.top, NULL, NULL, wc.hInstance, NULL);
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    return hwnd;
}

static int pump(void)
{
    MSG msg;
    while (PeekMessageA(&msg, NULL, 0, 0, PM_REMOVE)) {
        if (msg.message == WM_QUIT)
            return 0;
        TranslateMessage(&msg);
        DispatchMessageA(&msg);
    }
    return 1;
}

static void vk(WORD k, int down)
{
    INPUT in;
    memset(&in, 0, sizeof in);
    in.type = INPUT_KEYBOARD;
    in.ki.wVk = k;
    in.ki.dwFlags = down ? 0 : KEYEVENTF_KEYUP;
    SendInput(1, &in, sizeof in);
}

/* Brings HWND to the front. A process that is not in front may not take
 * the focus until it has sent input; F24 is a key nothing acts on. ALT is
 * the last resort, held around the call with F24 pressed inside it: a lone
 * ALT tap would leave the window's menu waiting for the next key. */
static int focus(HWND hwnd)
{
    int i;
    for (i = 0; i < 30; i++) {
        int how = i % 3;
        ShowWindow(hwnd, SW_RESTORE);
        if (how == 1) {
            vk(VK_F24, 1);
            vk(VK_F24, 0);
        }
        if (how == 2)
            vk(VK_MENU, 1);
        SetForegroundWindow(hwnd);
        if (how == 2) {
            vk(VK_F24, 1);
            vk(VK_F24, 0);
            vk(VK_MENU, 0);
        }
        Sleep(100);
        if (GetForegroundWindow() == hwnd) {
            printf("focus: %s\n", how == 0 ? "SetForegroundWindow" : how == 1 ? "after F24" : "with ALT held");
            return 1;
        }
    }
    printf("focus: FAILED\n");
    return 0;
}

/* ------------------------------------------------------------------ d3d9 */

typedef struct {
    float x, y, z, rhw;
    DWORD color;
} Vertex;

/* A HAL device on HWND, windowed or fullscreen 640x480, vsync off. */
static IDirect3DDevice9 *make_device(IDirect3D9 *d3d, HWND hwnd, int fullscreen, int verbose)
{
    IDirect3DDevice9 *dev = NULL;
    D3DPRESENT_PARAMETERS pp;
    DWORD vp[2] = {D3DCREATE_HARDWARE_VERTEXPROCESSING, D3DCREATE_SOFTWARE_VERTEXPROCESSING};
    const char *vpname[2] = {"hardware", "software"};
    HRESULT hr;
    int i;
    memset(&pp, 0, sizeof pp);
    pp.SwapEffect = D3DSWAPEFFECT_DISCARD;
    pp.hDeviceWindow = hwnd;
    pp.PresentationInterval = D3DPRESENT_INTERVAL_IMMEDIATE;
    if (fullscreen) {
        pp.Windowed = FALSE;
        pp.BackBufferWidth = 640;
        pp.BackBufferHeight = 480;
        pp.BackBufferFormat = D3DFMT_X8R8G8B8;
    } else {
        pp.Windowed = TRUE;
        pp.BackBufferFormat = D3DFMT_UNKNOWN;
    }
    for (i = 0; i < 2 && !dev; i++) {
        hr = IDirect3D9_CreateDevice(d3d, 0, D3DDEVTYPE_HAL, hwnd, vp[i], &pp, &dev);
        if (verbose || FAILED(hr))
            printf("CreateDevice(HAL, %s vertex processing, %s): hr=0x%08lx\n", vpname[i],
                   fullscreen ? "fullscreen 640x480" : "windowed", (unsigned long)hr);
    }
    if (dev)
        IDirect3DDevice9_SetRenderState(dev, D3DRS_CULLMODE, D3DCULL_NONE);
    return dev;
}

/* One frame: a turning triangle on purple, so a screenshot shows both the
 * clear and the rasterizer. */
static HRESULT draw_frame(IDirect3DDevice9 *dev, long long t)
{
    float k = (float)(t % 2000000) / 2000000.0f;
    Vertex v[3] = {
        {320.0f + 200.0f * (float)cos(6.2832 * k), 240.0f + 200.0f * (float)sin(6.2832 * k), 0.5f, 1.0f, 0xffffff00},
        {120.0f, 420.0f, 0.5f, 1.0f, 0xff00ffff},
        {520.0f, 420.0f, 0.5f, 1.0f, 0xffff00ff},
    };
    IDirect3DDevice9_Clear(dev, 0, NULL, D3DCLEAR_TARGET, D3DCOLOR_XRGB(40, 0, 90), 1.0f, 0);
    IDirect3DDevice9_BeginScene(dev);
    IDirect3DDevice9_SetFVF(dev, D3DFVF_XYZRHW | D3DFVF_DIFFUSE);
    IDirect3DDevice9_DrawPrimitiveUP(dev, D3DPT_TRIANGLELIST, 1, v, sizeof(Vertex));
    IDirect3DDevice9_EndScene(dev);
    return IDirect3DDevice9_Present(dev, NULL, NULL, NULL, NULL);
}

static int probe_d3d9(int seconds, int fullscreen)
{
    IDirect3D9 *d3d;
    IDirect3DDevice9 *dev;
    D3DCAPS9 caps;
    D3DDISPLAYMODE mode;
    HRESULT hr;
    HWND hwnd;
    UINT a, n;
    int frames = 0, lost = 0;
    long long t0, t;

    d3d = Direct3DCreate9(D3D_SDK_VERSION);
    printf("Direct3DCreate9: %s\n", d3d ? "ok" : "NULL");
    if (!d3d)
        return 2;
    n = IDirect3D9_GetAdapterCount(d3d);
    printf("adapters: %u\n", n);
    for (a = 0; a < n; a++) {
        D3DADAPTER_IDENTIFIER9 id;
        if (SUCCEEDED(IDirect3D9_GetAdapterIdentifier(d3d, a, 0, &id)))
            printf("adapter %u: %s (driver %s, vendor %04lx, device %04lx)\n", a, id.Description,
                   id.Driver, id.VendorId, id.DeviceId);
        if (SUCCEEDED(IDirect3D9_GetAdapterDisplayMode(d3d, a, &mode)))
            printf("adapter %u mode: %ux%u %u Hz format %d\n", a, mode.Width, mode.Height,
                   mode.RefreshRate, mode.Format);
        printf("adapter %u X8R8G8B8 modes: %u\n", a,
               IDirect3D9_GetAdapterModeCount(d3d, a, D3DFMT_X8R8G8B8));
    }
    hr = IDirect3D9_GetDeviceCaps(d3d, 0, D3DDEVTYPE_HAL, &caps);
    printf("HAL caps: hr=0x%08lx", (unsigned long)hr);
    if (SUCCEEDED(hr))
        printf(" hwtnl=%d vs=%lu.%lu ps=%lu.%lu maxtex=%lux%lu", !!(caps.DevCaps & D3DDEVCAPS_HWTRANSFORMANDLIGHT),
               (caps.VertexShaderVersion >> 8) & 0xff, caps.VertexShaderVersion & 0xff,
               (caps.PixelShaderVersion >> 8) & 0xff, caps.PixelShaderVersion & 0xff,
               caps.MaxTextureWidth, caps.MaxTextureHeight);
    printf("\n");

    hwnd = make_window("lr2probe d3d9", 640, 480, fullscreen ? WS_POPUP : WS_OVERLAPPEDWINDOW);
    dev = make_device(d3d, hwnd, fullscreen, 1);
    if (!dev) {
        IDirect3D9_Release(d3d);
        return 3;
    }
    t0 = now_us();
    while ((t = now_us()) - t0 < seconds * 1000000LL && pump()) {
        if (FAILED(draw_frame(dev, t - t0)))
            lost++;
        frames++;
    }
    printf("frames: %d in %d s (%.1f fps), failed presents: %d\n", frames, seconds,
           frames / (double)seconds, lost);
    IDirect3DDevice9_Release(dev);
    IDirect3D9_Release(d3d);
    DestroyWindow(hwnd);
    return frames > 0 && lost == 0 ? 0 : 4;
}

/* ----------------------------------------------------------------- sound */

/* WASAPI's enumerator, declared here so the C build needs no uuid lib */
DEFINE_GUID(probe_CLSID_MMDeviceEnumerator, 0xBCDE0395, 0xE52F, 0x467C, 0x8E, 0x3D, 0xC4, 0x57, 0x92,
            0x91, 0x69, 0x2E);
DEFINE_GUID(probe_IID_IMMDeviceEnumerator, 0xA95664D2, 0x9614, 0x4F35, 0xA7, 0x46, 0xDE, 0x8D, 0xB6,
            0x36, 0x17, 0xE6);

/* Only the vtable slots used, in the interface's order. */
typedef struct EnumVtbl {
    HRESULT(STDMETHODCALLTYPE *QueryInterface)(void *, REFIID, void **);
    ULONG(STDMETHODCALLTYPE *AddRef)(void *);
    ULONG(STDMETHODCALLTYPE *Release)(void *);
    HRESULT(STDMETHODCALLTYPE *EnumAudioEndpoints)(void *, int, DWORD, void **);
    HRESULT(STDMETHODCALLTYPE *GetDefaultAudioEndpoint)(void *, int, int, void **);
} EnumVtbl;
typedef struct ColVtbl {
    HRESULT(STDMETHODCALLTYPE *QueryInterface)(void *, REFIID, void **);
    ULONG(STDMETHODCALLTYPE *AddRef)(void *);
    ULONG(STDMETHODCALLTYPE *Release)(void *);
    HRESULT(STDMETHODCALLTYPE *GetCount)(void *, UINT *);
} ColVtbl;
typedef struct {
    EnumVtbl *v;
} Enumerator;
typedef struct {
    ColVtbl *v;
} Collection;
typedef struct {
    struct {
        void *q;
        void *a;
        ULONG(STDMETHODCALLTYPE *Release)(void *);
    } *v;
} Unknown;

static BOOL CALLBACK ds_enum(LPGUID guid, LPCSTR desc, LPCSTR module, LPVOID ctx)
{
    (void)ctx;
    printf("DirectSound device: %s (%s)%s\n", desc, module, guid ? "" : " [default]");
    return TRUE;
}

static int probe_sound(void)
{
    LPDIRECTSOUND8 ds = NULL;
    LPDIRECTSOUNDBUFFER prim = NULL, buf = NULL;
    DSBUFFERDESC bd;
    WAVEFORMATEX wf;
    HRESULT hr;
    Enumerator *en = NULL;
    int ok = 0;

    printf("waveOutGetNumDevs: %u\n", waveOutGetNumDevs());
    DirectSoundEnumerateA(ds_enum, NULL);

    CoInitialize(NULL);
    hr = CoCreateInstance(&probe_CLSID_MMDeviceEnumerator, NULL, CLSCTX_ALL,
                          &probe_IID_IMMDeviceEnumerator, (void **)&en);
    printf("MMDeviceEnumerator: hr=0x%08lx\n", (unsigned long)hr);
    if (SUCCEEDED(hr)) {
        Collection *col = NULL;
        Unknown *dev = NULL;
        UINT count = 0;
        /* eRender = 0, DEVICE_STATE_ACTIVE = 1, eConsole = 0 */
        hr = en->v->EnumAudioEndpoints(en, 0, 1, (void **)&col);
        if (SUCCEEDED(hr)) {
            col->v->GetCount(col, &count);
            col->v->Release(col);
        }
        printf("active render endpoints: %u (hr=0x%08lx)\n", count, (unsigned long)hr);
        hr = en->v->GetDefaultAudioEndpoint(en, 0, 0, (void **)&dev);
        printf("default render endpoint: hr=0x%08lx%s\n", (unsigned long)hr,
               hr == (HRESULT)0x80070490 ? " (E_NOTFOUND: no device)" : "");
        if (dev)
            dev->v->Release(dev);
        en->v->Release(en);
    }

    hr = DirectSoundCreate8(NULL, &ds, NULL);
    printf("DirectSoundCreate8(default): hr=0x%08lx\n", (unsigned long)hr);
    if (FAILED(hr))
        return 2;
    hr = IDirectSound8_SetCooperativeLevel(ds, GetDesktopWindow(), DSSCL_PRIORITY);
    printf("SetCooperativeLevel(PRIORITY): hr=0x%08lx\n", (unsigned long)hr);
    memset(&bd, 0, sizeof bd);
    bd.dwSize = sizeof bd;
    bd.dwFlags = DSBCAPS_PRIMARYBUFFER;
    hr = IDirectSound8_CreateSoundBuffer(ds, &bd, &prim, NULL);
    printf("primary buffer: hr=0x%08lx\n", (unsigned long)hr);

    memset(&wf, 0, sizeof wf);
    wf.wFormatTag = WAVE_FORMAT_PCM;
    wf.nChannels = 2;
    wf.nSamplesPerSec = 44100;
    wf.wBitsPerSample = 16;
    wf.nBlockAlign = 4;
    wf.nAvgBytesPerSec = 44100 * 4;
    memset(&bd, 0, sizeof bd);
    bd.dwSize = sizeof bd;
    bd.dwFlags = DSBCAPS_GETCURRENTPOSITION2 | DSBCAPS_GLOBALFOCUS;
    bd.dwBufferBytes = wf.nAvgBytesPerSec;
    bd.lpwfxFormat = &wf;
    hr = IDirectSound8_CreateSoundBuffer(ds, &bd, &buf, NULL);
    printf("secondary buffer: hr=0x%08lx\n", (unsigned long)hr);
    if (SUCCEEDED(hr)) {
        void *p1, *p2;
        DWORD n1, n2, play0 = 0, play1 = 0, w;
        if (SUCCEEDED(IDirectSoundBuffer_Lock(buf, 0, 0, &p1, &n1, &p2, &n2, DSBLOCK_ENTIREBUFFER))) {
            short *s = (short *)p1;
            DWORD i;
            for (i = 0; i < n1 / 2; i++)
                s[i] = (short)(3000 * sin(6.2832 * 440.0 * (i / 2) / 44100.0));
            IDirectSoundBuffer_Unlock(buf, p1, n1, p2, n2);
        }
        hr = IDirectSoundBuffer_Play(buf, 0, 0, DSBPLAY_LOOPING);
        printf("Play: hr=0x%08lx\n", (unsigned long)hr);
        IDirectSoundBuffer_GetCurrentPosition(buf, &play0, &w);
        Sleep(500);
        IDirectSoundBuffer_GetCurrentPosition(buf, &play1, &w);
        /* the play cursor moves only when a device clocks the buffer */
        printf("play cursor after 500 ms: %lu -> %lu bytes (%.0f ms)\n", play0, play1,
               ((play1 + bd.dwBufferBytes - play0) % bd.dwBufferBytes) * 1000.0 / wf.nAvgBytesPerSec);
        ok = play1 != play0;
        IDirectSoundBuffer_Stop(buf);
        IDirectSoundBuffer_Release(buf);
    }
    if (prim)
        IDirectSoundBuffer_Release(prim);
    IDirectSound8_Release(ds);
    return ok ? 0 : 3;
}

/* ------------------------------------------------------------------- acp */

static int CALLBACK font_found(const LOGFONTW *lf, const TEXTMETRICW *tm, DWORD type, LPARAM found)
{
    (void)lf;
    (void)tm;
    (void)type;
    *(int *)found = 1;
    return 0;
}

static int has_font(const wchar_t *face)
{
    LOGFONTW lf;
    int found = 0;
    HDC dc = GetDC(NULL);
    memset(&lf, 0, sizeof lf);
    lf.lfCharSet = SHIFTJIS_CHARSET;
    wcsncpy(lf.lfFaceName, face, LF_FACESIZE - 1);
    EnumFontFamiliesExW(dc, &lf, (FONTENUMPROCW)font_found, (LPARAM)&found, 0);
    ReleaseDC(NULL, dc);
    return found;
}

static int probe_acp(void)
{
    /* "テスト" (test) in Shift-JIS, read as the process's ANSI code page */
    const char sjis[] = "\x83\x65\x83\x58\x83\x67";
    const wchar_t want[] = {0x30c6, 0x30b9, 0x30c8, 0};
    wchar_t got[16];
    char utf8[64];
    int n;
    printf("GetACP: %u\nGetOEMCP: %u\n", GetACP(), GetOEMCP());
    printf("system default LCID: 0x%04lx\nuser default LCID: 0x%04lx\nthread locale: 0x%04lx\n",
           GetSystemDefaultLCID(), GetUserDefaultLCID(), GetThreadLocale());
    printf("system UI language: 0x%04x\nuser UI language: 0x%04x\n", GetSystemDefaultUILanguage(),
           GetUserDefaultUILanguage());
    n = MultiByteToWideChar(CP_ACP, 0, sjis, -1, got, 16);
    WideCharToMultiByte(CP_UTF8, 0, got, -1, utf8, sizeof utf8, NULL, NULL);
    printf("Shift-JIS bytes read as ANSI: %d chars, %s\n", n, wcscmp(got, want) == 0 ? "Japanese (ok)" : "not Japanese");
    printf("font MS Gothic (Shift-JIS charset): %s\n", has_font(L"MS Gothic") ? "yes" : "no");
    printf("font \xef\xbc\xad\xef\xbc\xb3 \xe3\x82\xb4\xe3\x82\xb7\xe3\x83\x83\xe3\x82\xaf (Shift-JIS charset): %s\n",
           has_font(L"\xff2d\xff33 \x30b4\x30b7\x30c3\x30af") ? "yes" : "no");
    return GetACP() == 932 ? 0 : 1;
}

/* ----------------------------------------------------------------- input */

static int probe_recv(int n, const char *csv, int render)
{
    HWND hwnd = make_window(RECV_TITLE, 640, 480, WS_OVERLAPPEDWINDOW);
    IDirect3D9 *d3d = NULL;
    IDirect3DDevice9 *dev = NULL;
    long long frames = 0, t_start = now_us();
    LPDIRECTINPUT8 di = NULL;
    LPDIRECTINPUTDEVICE8 kb = NULL;
    DIPROPDWORD prop;
    HRESULT hr;
    FILE *f;
    int i, downs_m = 0, downs_d = 0;
    long long last = now_us();

    timeBeginPeriod(1);
    if (render && (d3d = Direct3DCreate9(D3D_SDK_VERSION)) != NULL)
        dev = make_device(d3d, hwnd, 0, 0);
    if (render)
        printf("drawing with D3D9: %s\n", dev ? "yes" : "NO");
    hr = DirectInput8Create(GetModuleHandleA(NULL), DIRECTINPUT_VERSION, &IID_IDirectInput8A, (void **)&di, NULL);
    printf("DirectInput8Create: hr=0x%08lx\n", (unsigned long)hr);
    if (SUCCEEDED(hr)) {
        IDirectInput8_CreateDevice(di, &GUID_SysKeyboard, &kb, NULL);
        IDirectInputDevice8_SetDataFormat(kb, &c_dfDIKeyboard);
        IDirectInputDevice8_SetCooperativeLevel(kb, hwnd, DISCL_FOREGROUND | DISCL_NONEXCLUSIVE);
        memset(&prop, 0, sizeof prop);
        prop.diph.dwSize = sizeof prop;
        prop.diph.dwHeaderSize = sizeof prop.diph;
        prop.diph.dwHow = DIPH_DEVICE;
        prop.dwData = 256;
        IDirectInputDevice8_SetProperty(kb, DIPROP_BUFFERSIZE, &prop.diph);
    }
    /* runs until N downs arrive both ways, or 20 s pass without a key */
    while (pump() && (downs_m < n || (kb && downs_d < n)) && now_us() - last < 20000000LL) {
        if (kb) {
            DIDEVICEOBJECTDATA d[64];
            DWORD k = 64;
            hr = IDirectInputDevice8_GetDeviceData(kb, sizeof d[0], d, &k, 0);
            if (hr == DIERR_INPUTLOST || hr == DIERR_NOTACQUIRED) {
                IDirectInputDevice8_Acquire(kb);
            } else if (SUCCEEDED(hr)) {
                DWORD j;
                for (j = 0; j < k; j++) {
                    record('d', d[j].dwOfs, (d[j].dwData & 0x80) != 0, d[j].dwTimeStamp);
                    downs_d += d[j].dwOfs == SCAN_Z && (d[j].dwData & 0x80);
                }
            }
        }
        if (n_events) {
            downs_m = 0;
            for (i = 0; i < n_events; i++)
                downs_m += events[i].src == 'm' && events[i].code == SCAN_Z && events[i].down;
            last = events[n_events - 1].t_us;
        }
        /* a game polls once a frame; without a device this polls about
         * every 0.5 ms */
        if (dev) {
            draw_frame(dev, now_us());
            frames++;
        } else {
            long long until = now_us() + 500;
            while (now_us() < until)
                YieldProcessor();
        }
    }
    printf("received: %d downs by message, %d by DirectInput, %d events\n", downs_m, downs_d, n_events);
    if (dev)
        printf("frames: %lld, %.1f fps\n", frames, frames * 1e6 / (double)(now_us() - t_start));
    f = fopen(csv, "w");
    if (!f)
        return 2;
    fprintf(f, "src,t_us,scan,down,stamp_ms\n");
    for (i = 0; i < n_events; i++)
        fprintf(f, "%c,%lld,%u,%d,%u\n", events[i].src, events[i].t_us, events[i].code, events[i].down,
                events[i].stamp_ms);
    fclose(f);
    if (kb) {
        IDirectInputDevice8_Unacquire(kb);
        IDirectInputDevice8_Release(kb);
    }
    if (di)
        IDirectInput8_Release(di);
    if (dev)
        IDirect3DDevice9_Release(dev);
    if (d3d)
        IDirect3D9_Release(d3d);
    DestroyWindow(hwnd);
    return downs_m >= n ? 0 : 3;
}

static void wait_until(long long t)
{
    while (t - now_us() > 2000)
        Sleep(1);
    while (now_us() < t)
        YieldProcessor();
}

static void key(WORD scan, int down)
{
    INPUT in;
    memset(&in, 0, sizeof in);
    in.type = INPUT_KEYBOARD;
    in.ki.wScan = scan;
    in.ki.dwFlags = KEYEVENTF_SCANCODE | (down ? 0 : KEYEVENTF_KEYUP);
    SendInput(1, &in, sizeof in);
}

static int probe_send(int n, int period_ms, const char *csv)
{
    HWND hwnd = NULL;
    FILE *f;
    int i;
    long long t0;
    long long *sched = calloc(n, sizeof *sched), *sent = calloc(n, sizeof *sent);

    for (i = 0; i < 100 && !(hwnd = FindWindowA("lr2probe", RECV_TITLE)); i++)
        Sleep(100);
    if (!hwnd) {
        printf("no receiver window\n");
        return 2;
    }
    focus(hwnd);
    timeBeginPeriod(1);
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
    Sleep(500);
    t0 = now_us() + 200000;
    for (i = 0; i < n; i++) {
        sched[i] = t0 + (long long)i * period_ms * 1000;
        wait_until(sched[i]);
        key(SCAN_Z, 1);
        sent[i] = now_us();
        wait_until(sched[i] + 30000);
        key(SCAN_Z, 0);
    }
    timeEndPeriod(1);
    f = fopen(csv, "w");
    if (!f)
        return 3;
    fprintf(f, "i,sched_us,sent_us\n");
    for (i = 0; i < n; i++)
        fprintf(f, "%d,%lld,%lld\n", i, sched[i], sent[i]);
    fclose(f);
    printf("sent %d presses, %d ms apart\n", n, period_ms);
    return 0;
}

/* ------------------------------------------------------------- dialogs */

typedef struct {
    HWND dlg;
    int id, page;
    HWND found;
} FindCtl;

static BOOL CALLBACK find_ctl(HWND h, LPARAM l)
{
    FindCtl *f = (FindCtl *)l;
    HWND parent = GetParent(h);
    if (GetDlgCtrlID(h) != f->id)
        return TRUE;
    /* p/ID: a control on a page (a child dialog) that is shown; ID: one
     * of the dialog's own */
    if (f->page ? (parent != f->dlg && IsWindowVisible(parent)) : parent == f->dlg) {
        f->found = h;
        return FALSE;
    }
    return TRUE;
}

static HWND ctl(HWND dlg, const char *spec)
{
    FindCtl f;
    f.dlg = dlg;
    f.page = !strncmp(spec, "p/", 2);
    f.id = atoi(spec + (f.page ? 2 : 0));
    f.found = NULL;
    EnumChildWindows(dlg, find_ctl, (LPARAM)&f);
    if (!f.found)
        fprintf(stderr, "no control %s\n", spec);
    return f.found;
}

static BOOL CALLBACK dump_ctl(HWND h, LPARAM l)
{
    HWND dlg = (HWND)l;
    wchar_t text[512], cls[64];
    char t8[1536], c8[192];
    DWORD_PTR check = 0;
    SendMessageTimeoutW(h, WM_GETTEXT, 512, (LPARAM)text, SMTO_ABORTIFHUNG, 1000, NULL);
    text[511] = 0;
    GetClassNameW(h, cls, 64);
    WideCharToMultiByte(CP_UTF8, 0, text, -1, t8, sizeof t8, NULL, NULL);
    WideCharToMultiByte(CP_UTF8, 0, cls, -1, c8, sizeof c8, NULL, NULL);
    if (!wcscmp(cls, L"Button"))
        SendMessageTimeoutW(h, BM_GETCHECK, 0, 0, SMTO_ABORTIFHUNG, 1000, &check);
    printf("%s%d [%s] \"%s\"%s%s%s\n", GetParent(h) == dlg ? "" : "  p/", GetDlgCtrlID(h), c8, t8,
           check == BST_CHECKED ? " checked" : "", IsWindowVisible(h) ? "" : " hidden",
           IsWindowEnabled(h) ? "" : " disabled");
    return TRUE;
}

static void click_at(int x, int y)
{
    INPUT in[3];
    int sw = GetSystemMetrics(SM_CXSCREEN), sh = GetSystemMetrics(SM_CYSCREEN);
    memset(in, 0, sizeof in);
    in[0].type = in[1].type = in[2].type = INPUT_MOUSE;
    in[0].mi.dx = (x * 65535) / (sw - 1);
    in[0].mi.dy = (y * 65535) / (sh - 1);
    in[0].mi.dwFlags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE;
    in[1].mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
    in[2].mi.dwFlags = MOUSEEVENTF_LEFTUP;
    SendInput(3, in, sizeof(INPUT));
}

static int ui(int argc, char **argv)
{
    /* argv: TITLE ACTION ARGS; the title and a text in UTF-16, as given
     * (the char argv has them in the ANSI code page) */
    int wargc = 0;
    wchar_t **wargv = CommandLineToArgvW(GetCommandLineW(), &wargc);
    const wchar_t *title = wargc > 2 ? wargv[2] : L"";
    HWND dlg, h;
    int i;
    /* TITLE, or a window handle as 0x... */
    if (!wcsncmp(title, L"0x", 2))
        dlg = (HWND)(INT_PTR)wcstoul(title + 2, NULL, 16);
    else
        for (i = 0; i < 100 && !(dlg = FindWindowW(NULL, title)); i++)
            Sleep(100);
    if (!dlg) {
        fprintf(stderr, "no window %s\n", argv[0]);
        return 2;
    }
    if (!strcmp(argv[1], "dump")) {
        EnumChildWindows(dlg, dump_ctl, (LPARAM)dlg);
        return 0;
    }
    if (!strcmp(argv[1], "click") && argc >= 3) {
        if (!(h = ctl(dlg, argv[2])))
            return 3;
        PostMessageW(h, BM_CLICK, 0, 0);
        return 0;
    }
    if (!strcmp(argv[1], "check") && argc >= 4) {
        DWORD_PTR now = 0;
        if (!(h = ctl(dlg, argv[2])))
            return 3;
        SendMessageTimeoutW(h, BM_GETCHECK, 0, 0, SMTO_ABORTIFHUNG, 1000, &now);
        if ((now == BST_CHECKED) != (atoi(argv[3]) != 0))
            SendMessageTimeoutW(h, BM_CLICK, 0, 0, SMTO_ABORTIFHUNG, 2000, NULL);
        return 0;
    }
    if (!strcmp(argv[1], "text") && argc >= 4) {
        if (!(h = ctl(dlg, argv[2])) || wargc < 6)
            return 3;
        SendMessageTimeoutW(h, WM_SETTEXT, 0, (LPARAM)wargv[5], SMTO_ABORTIFHUNG, 2000, NULL);
        return 0;
    }
    if (!strcmp(argv[1], "items") && argc >= 3) {
        DWORD_PTR n = 0, len = 0, k;
        if (!(h = ctl(dlg, argv[2])))
            return 3;
        SendMessageTimeoutW(h, CB_GETCOUNT, 0, 0, SMTO_ABORTIFHUNG, 1000, &n);
        SendMessageTimeoutW(h, CB_GETCURSEL, 0, 0, SMTO_ABORTIFHUNG, 1000, &k);
        printf("%lu items, selected %ld\n", (unsigned long)n, (long)k);
        for (k = 0; k < n; k++) {
            wchar_t item[512];
            char i8[1536];
            SendMessageTimeoutW(h, CB_GETLBTEXTLEN, k, 0, SMTO_ABORTIFHUNG, 1000, &len);
            if (len >= 512)
                continue;
            SendMessageTimeoutW(h, CB_GETLBTEXT, k, (LPARAM)item, SMTO_ABORTIFHUNG, 1000, NULL);
            WideCharToMultiByte(CP_UTF8, 0, item, -1, i8, sizeof i8, NULL, NULL);
            printf("  %lu \"%s\"\n", (unsigned long)k, i8);
        }
        return 0;
    }
    if (!strcmp(argv[1], "select") && argc >= 4) {
        DWORD_PTR k = (DWORD_PTR)-1;
        if (!(h = ctl(dlg, argv[2])) || wargc < 6)
            return 3;
        SendMessageTimeoutW(h, CB_SELECTSTRING, (WPARAM)-1, (LPARAM)wargv[5], SMTO_ABORTIFHUNG, 1000, &k);
        printf("selected item %ld\n", (long)k);
        if (k == (DWORD_PTR)-1)
            return 4;
        SendMessageTimeoutW(GetParent(h), WM_COMMAND, MAKEWPARAM(GetDlgCtrlID(h), CBN_SELCHANGE), (LPARAM)h,
                            SMTO_ABORTIFHUNG, 2000, NULL);
        return 0;
    }
    if (!strcmp(argv[1], "mouse") && argc >= 4) {
        RECT r;
        focus(dlg);
        GetWindowRect(dlg, &r);
        click_at(r.left + atoi(argv[2]), r.top + atoi(argv[3]));
        Sleep(200);
        return 0;
    }
    if (!strcmp(argv[1], "key") && argc >= 3) {
        focus(dlg);
        vk((WORD)strtol(argv[2], NULL, 16), 1);
        vk((WORD)strtol(argv[2], NULL, 16), 0);
        Sleep(100);
        return 0;
    }
    fprintf(stderr, "unknown ui action %s\n", argv[1]);
    return 64;
}

/* ----------------------------------------------------- drag and drop */

/* A drag of PATH as Explorer makes it (the shell's own data object), the
 * mouse moved and let go by SendInput while DoDragDrop runs: what a person
 * dragging a folder onto the window does. */
DEFINE_GUID(probe_BHID_DataObject, 0xB8C0BD9F, 0xED24, 0x455C, 0x83, 0xE6, 0xD5, 0x39, 0x0C, 0x4F, 0xE8, 0xC4);

typedef struct {
    IDropSourceVtbl *lpVtbl;
} DropSource;

static HRESULT STDMETHODCALLTYPE ds_qi(IDropSource *t, REFIID r, void **o)
{
    if (IsEqualIID(r, &IID_IUnknown) || IsEqualIID(r, &IID_IDropSource)) {
        *o = t;
        return S_OK;
    }
    *o = NULL;
    return E_NOINTERFACE;
}
static ULONG STDMETHODCALLTYPE ds_ref(IDropSource *t)
{
    (void)t;
    return 1;
}
static HRESULT STDMETHODCALLTYPE ds_continue(IDropSource *t, BOOL esc, DWORD keys)
{
    (void)t;
    if (esc)
        return DRAGDROP_S_CANCEL;
    return keys & MK_LBUTTON ? S_OK : DRAGDROP_S_DROP;
}
static HRESULT STDMETHODCALLTYPE ds_feedback(IDropSource *t, DWORD e)
{
    (void)t;
    (void)e;
    return DRAGDROP_S_USEDEFAULTCURSORS;
}
static IDropSourceVtbl ds_vtbl = {ds_qi, ds_ref, ds_ref, ds_continue, ds_feedback};

typedef struct {
    int x0, y0, x1, y1;
} DragPath;

static void mouse_to(int x, int y, DWORD extra)
{
    INPUT in;
    int sw = GetSystemMetrics(SM_CXSCREEN), sh = GetSystemMetrics(SM_CYSCREEN);
    memset(&in, 0, sizeof in);
    in.type = INPUT_MOUSE;
    in.mi.dx = (x * 65535) / (sw - 1);
    in.mi.dy = (y * 65535) / (sh - 1);
    in.mi.dwFlags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | extra;
    SendInput(1, &in, sizeof in);
}

static DWORD WINAPI drag_mouse(LPVOID arg)
{
    DragPath *d = (DragPath *)arg;
    int i;
    Sleep(300);
    for (i = 1; i <= 30; i++) {
        mouse_to(d->x0 + (d->x1 - d->x0) * i / 30, d->y0 + (d->y1 - d->y0) * i / 30, 0);
        Sleep(30);
    }
    Sleep(500);
    mouse_to(d->x1, d->y1, MOUSEEVENTF_LEFTUP);
    return 0;
}

static int probe_drop(HWND target, int x, int y, const wchar_t *path)
{
    IShellItem *item = NULL;
    IDataObject *data = NULL;
    DropSource src = {&ds_vtbl};
    DragPath d;
    RECT r;
    DWORD effect = 0;
    HRESULT hr;
    HWND own;
    HANDLE th;

    OleInitialize(NULL);
    hr = SHCreateItemFromParsingName(path, NULL, &IID_IShellItem, (void **)&item);
    if (SUCCEEDED(hr))
        hr = item->lpVtbl->BindToHandler(item, NULL, &probe_BHID_DataObject, &IID_IDataObject, (void **)&data);
    printf("data object for the path: hr=0x%08lx\n", (unsigned long)hr);
    if (FAILED(hr))
        return 2;
    /* the drag starts on a window of ours, with the button down there */
    own = make_window("lr2probe drag", 120, 80, WS_POPUP | WS_BORDER);
    SetWindowPos(own, HWND_TOPMOST, GetSystemMetrics(SM_CXSCREEN) - 140, 20, 120, 80, SWP_SHOWWINDOW);
    focus(own);
    GetWindowRect(target, &r);
    d.x0 = GetSystemMetrics(SM_CXSCREEN) - 80;
    d.y0 = 60;
    d.x1 = r.left + x;
    d.y1 = r.top + y;
    mouse_to(d.x0, d.y0, 0);
    Sleep(100);
    mouse_to(d.x0, d.y0, MOUSEEVENTF_LEFTDOWN);
    Sleep(100);
    th = CreateThread(NULL, 0, drag_mouse, &d, 0, NULL);
    hr = DoDragDrop(data, (IDropSource *)&src, DROPEFFECT_COPY | DROPEFFECT_LINK | DROPEFFECT_MOVE, &effect);
    WaitForSingleObject(th, 5000);
    printf("DoDragDrop: hr=0x%08lx effect %lu%s\n", (unsigned long)hr, effect,
           hr == DRAGDROP_S_DROP ? " (dropped)" : "");
    data->lpVtbl->Release(data);
    item->lpVtbl->Release(item);
    DestroyWindow(own);
    OleUninitialize();
    return hr == DRAGDROP_S_DROP ? 0 : 3;
}

static BOOL CALLBACK list_window(HWND h, LPARAM l)
{
    wchar_t title[256], cls[128];
    char t8[768], c8[384];
    DWORD pid = 0;
    RECT r;
    (void)l;
    if (!IsWindowVisible(h))
        return TRUE;
    GetWindowTextW(h, title, 256);
    GetClassNameW(h, cls, 128);
    GetWindowThreadProcessId(h, &pid);
    GetWindowRect(h, &r);
    WideCharToMultiByte(CP_UTF8, 0, title, -1, t8, sizeof t8, NULL, NULL);
    WideCharToMultiByte(CP_UTF8, 0, cls, -1, c8, sizeof c8, NULL, NULL);
    printf("0x%p pid %lu [%s] \"%s\" %ld,%ld %ldx%ld%s\n", (void *)h, pid, c8, t8, r.left, r.top,
           r.right - r.left, r.bottom - r.top, h == GetForegroundWindow() ? " (foreground)" : "");
    return TRUE;
}

int main(int argc, char **argv)
{
    QueryPerformanceFrequency(&qpf);
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc >= 3 && !strcmp(argv[1], "d3d9"))
        return probe_d3d9(atoi(argv[2]), argc >= 4 && !strcmp(argv[3], "fullscreen"));
    if (argc >= 2 && !strcmp(argv[1], "sound"))
        return probe_sound();
    if (argc >= 2 && !strcmp(argv[1], "acp")) {
        /* a file, for a run under another launcher that drops the console */
        if (argc >= 3 && !freopen(argv[2], "w", stdout))
            return 2;
        return probe_acp();
    }
    if (argc >= 4 && !strcmp(argv[1], "recv"))
        return probe_recv(atoi(argv[2]), argv[3], argc >= 5 && !strcmp(argv[4], "d3d9"));
    if (argc >= 5 && !strcmp(argv[1], "send"))
        return probe_send(atoi(argv[2]), atoi(argv[3]), argv[4]);
    if (argc >= 6 && !strcmp(argv[1], "drop")) {
        int wargc = 0;
        wchar_t **wargv = CommandLineToArgvW(GetCommandLineW(), &wargc);
        HWND t = !wcsncmp(wargv[2], L"0x", 2) ? (HWND)(INT_PTR)wcstoul(wargv[2] + 2, NULL, 16)
                                                : FindWindowW(NULL, wargv[2]);
        if (!t) {
            fprintf(stderr, "no window\n");
            return 2;
        }
        return probe_drop(t, atoi(argv[3]), atoi(argv[4]), wargv[5]);
    }
    if (argc >= 4 && !strcmp(argv[1], "ui"))
        return ui(argc - 2, argv + 2);
    if (argc >= 2 && !strcmp(argv[1], "windows"))
        return !EnumWindows(list_window, 0);
    if (argc >= 3 && !strcmp(argv[1], "focus")) {
        HWND h = FindWindowA(NULL, argv[2]);
        return h && focus(h) ? 0 : 1;
    }
    fprintf(stderr, "usage: lr2probe d3d9 SECONDS [fullscreen] | sound | acp [OUT] | recv N CSV [d3d9] | send N PERIOD_MS CSV | focus TITLE | windows | ui TITLE ACTION ... | drop TITLE X Y PATH\n");
    return 64;
}
