# Anixart Arch

An independent native Linux desktop client experiment for Anixart, written from scratch in Rust with GTK4/libadwaita.

**No Waydroid, Android emulator, APK execution, Electron, or Chromium runtime is used.** GTK/GDK provides normal Wayland/X11 integration. WebKitGTK is used only as an in-process native web-content widget for episode providers that Anixart itself treats as iframe players.

> Unofficial project. Not affiliated with or endorsed by Anixart. The project does not redistribute the Android APK, official artwork, or decompiled application source.

## 0.4.1 desktop UI

0.4.1 keeps the working native playback stack from 0.3.x and changes the application shell into one-window navigation:

```text
Home / poster grid
    ↓ select release
Release details
    ↓ Play
Voiceover → source → episode selection
    ↓ play episode
Dedicated WebKitGTK/mpv playback window
```

The Home/Discover parser also understands Anixart's wrapper objects. In Beta 21, `discover/interesting` uses an `Interesting` navigation/banner model with its own `id`, `title`, `image` and `action`. That wrapper `id` is not necessarily a release ID. 0.4.1 resolves the real release target from the `action` string and additionally accepts explicit `release_id` / `releaseId` or nested `release` schemas. This fixes empty voiceover lists caused by sending Discover wrapper IDs to the episode API.

Implemented:

- native GTK4/libadwaita Wayland/X11 application;
- one-window `GtkStack` navigation for Home, release details and playback selection;
- compact poster grid with asynchronous artwork loading;
- bounded desktop detail layout: artwork left, scrollable information right;
- release Search / Discover / details;
- voiceover/subtitle artwork and filtering;
- voiceover → source → episode API workflow;
- direct-stream mpv and iframe/provider WebKitGTK playback;
- Settings + separate Developer tools menu;
- UI-only Filter/category/navigation placeholders for later wiring;
- OpenGL GSK renderer default on Wayland while respecting explicit overrides;
- stale-response guards and visible API errors on the active playback page.

## Dependencies on Arch / CachyOS

The local PKGBUILD installs these automatically. For manual source builds:

```bash
sudo pacman -S --needed base-devel rust gtk4 libadwaita webkitgtk-6.0 mpv \
  gst-plugins-good gst-plugins-bad gst-libav
```

Optional:

```bash
sudo pacman -S --needed yt-dlp
```

`yt-dlp` is not required for iframe playback and is not used to bypass a provider page. It is merely available to mpv for URLs that yt-dlp legitimately supports.

## Build/install on Arch

Use the supplied local AUR directory:

```bash
makepkg -Csi
anixart
```

## Playback UI

1. Search/select release.
2. Select voiceover/subtitles.
3. Select source.
4. Select episode.
5. The primary button changes automatically:
   - **Play in mpv** for recognized direct-media URLs.
   - **Play in app** for iframe/provider URLs.
6. Double-clicking an episode uses the same automatic router.

**Open URL (debug)** deliberately opens the raw provider link as a top-level page. Iframe-only services may show a 404 there; that does not imply the embedded player is broken.

## CLI

```bash
anixart
anixart --help
anixart --version
anixart --api-check
anixart --play 'https://example.invalid/master.m3u8'
```

## License

The new source in this repository is GPL-3.0-or-later. This license applies only to this project's original code and does not grant rights to Anixart's APK, branding, service, or third-party media.


## 0.3.2 compile compatibility

Rust/gtk-rs exposes both GTK `WidgetExt::settings()` and WebKit `WebViewExt::settings()` on `WebView`. 0.3.2 explicitly calls the WebKit trait method, fixing E0034/E0282 on current Arch/CachyOS toolchains while retaining the 0.3.1 provider media settings.

## Provider playback compatibility (0.3.1)

The embedded player now mirrors the relevant Android WebView behaviour more closely: JavaScript, DOM/local storage, MediaSource, Media Capabilities, inline/autoplay media, fullscreen, WebAudio and site-specific quirks are enabled. Provider HTML is loaded with a real HTTP(S) base URI instead of `about:blank`, and an Android-compatible WebView user agent is used for provider compatibility.

Run `anixart --media-check` to verify the local GStreamer decoders/demuxers used by WebKitGTK. `GST_DEBUG=2 anixart` enables useful media-pipeline diagnostics.


## Fullscreen / Wayland performance (0.3.4)

0.3.4 makes the empirically verified fullscreen fix the default. A Wayland
trace at 175% fractional scaling showed GTK's Vulkan renderer rejecting the
WebKit XR24 DMABUF modifier, then repeatedly importing/downloading the same
large frames through OpenGL as a fallback. The result was severe fullscreen
video and pointer stutter.

On Wayland, Anixart Native now sets `GSK_RENDERER=gl` **only when the variable
is not already set by the user**. GTK's GL/EGL path can import the provider
DMABUF directly on the affected stack, eliminating the expensive
Vulkan→GL fallback loop. Desktop shortcuts and a plain `anixart` command get
the same behavior automatically.

Explicit overrides remain available:

```bash
GSK_RENDERER=vulkan anixart   # future regression/fix testing
GSK_RENDERER=gl anixart       # force the known-good path explicitly
```

`GraphicsOffload` remains enabled as a hint because it can still provide a
direct Wayland fast path at integer scaling. When fractional geometry prevents
offload, the normal GL composition path is now the expected fast fallback.

The package also depends on `gst-plugin-va` so GStreamer can use VA-API
hardware decoders. You still need the matching GPU VA driver. Examples:

```bash
# Modern Intel / Arc
sudo pacman -S --needed intel-media-driver libva-utils

# AMD/Mesa
sudo pacman -S --needed libva-mesa-driver libva-utils
```

Inspect the effective media stack with:

```bash
anixart --media-check
```

For the fullscreen path specifically:

```bash
GDK_DEBUG=offload,dmabuf anixart 2>&1 | tee ~/anixart-offload.log
```

Two A/B troubleshooting switches are available. They are intentionally **not**
the default because they disable fast paths:

```bash
anixart --webkit-no-dmabuf
anixart --webkit-no-compositing
```

If disabling DMABUF unexpectedly makes a broken driver/compositor combination
smoother, that result is diagnostically useful; do not assume it is the ideal
long-term configuration.
