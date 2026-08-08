use gtk::prelude::*;
use std::process::{Command, Stdio};
use thiserror::Error;
use url::Url;
use webkit6::prelude::{SettingsExt, WebViewExt};

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("invalid media URL: {0}")]
    InvalidUrl(String),
    #[error("only http:// and https:// URLs are accepted")]
    UnsupportedScheme,
    #[error("mpv is not installed or could not be started: {0}")]
    MpvSpawn(#[source] std::io::Error),
    #[error("the system browser could not be started through gio: {0}")]
    BrowserSpawn(#[source] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackRoute {
    Mpv,
    EmbeddedWeb,
}

pub struct MpvBackend;
pub struct BrowserBackend;
pub struct EmbeddedWebBackend;

impl MpvBackend {
    pub fn play(input: &str) -> Result<(), PlayerError> {
        let url = checked_url(input)?;

        Command::new("mpv")
            .arg("--force-window=yes")
            .arg("--keep-open=no")
            .arg("--ytdl=yes")
            .arg("--")
            .arg(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(PlayerError::MpvSpawn)?;

        Ok(())
    }
}

impl BrowserBackend {
    /// Opens the raw provider URL as a top-level browser page.
    ///
    /// This is deliberately a diagnostics fallback. Several iframe-only
    /// providers reject top-level navigation even though embedding works.
    pub fn open(input: &str) -> Result<(), PlayerError> {
        let url = checked_url(input)?;

        Command::new("gio")
            .arg("open")
            .arg(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(PlayerError::BrowserSpawn)?;

        Ok(())
    }
}

impl EmbeddedWebBackend {
    /// Opens an Anixart-style provider URL inside a native GTK/WebKit window.
    ///
    /// The Android reference application wraps these URLs in an iframe rather
    /// than navigating to them as a top-level browser document. Reproducing
    /// that distinction is important for providers that intentionally return
    /// a 404-like page when opened directly.
    pub fn open(
        parent: &adw::ApplicationWindow,
        input: &str,
        title: &str,
    ) -> Result<(), PlayerError> {
        let url = checked_url(input)?;
        let escaped_url = escape_html_attr(url.as_str());
        let escaped_title = escape_html_text(title);
        // Keep the wrapper intentionally close to Anixart's bundled
        // assets/player_default.html. Provider pages often behave differently
        // when navigated top-level versus when embedded by an app WebView.
        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{escaped_title}</title>
<style>
body {{ margin: 0; width: 100%; height: 100%; background-color: #000; overflow: hidden; }}
html {{ width: 100%; height: 100%; background-color: #000; }}
.embed-container iframe,
.embed-container object,
.embed-container embed {{
    position: absolute;
    top: 0;
    left: 0;
    width: 100% !important;
    height: 100% !important;
    border: 0;
}}
</style>
</head>
<body>
<div class="embed-container">
<iframe src="{escaped_url}" style="width:100%;" frameborder="0"
        allow="autoplay; fullscreen; encrypted-media; picture-in-picture"
        allowfullscreen></iframe>
</div>
</body>
</html>"#
        );

        let window = gtk::Window::builder()
            .title(title)
            .transient_for(parent)
            .default_width(1280)
            .default_height(720)
            .build();

        let webview = webkit6::WebView::new();
        webview.set_hexpand(true);
        webview.set_vexpand(true);

        // Android's reference WebView explicitly enables the web platform
        // features used by these provider players. WebKitGTK defaults are more
        // conservative and can make old Flowplayer-style sites incorrectly
        // fall back to their long-dead Flash error page.
        if let Some(settings) = WebViewExt::settings(&webview) {
            // Be explicit even though current WebKitGTK normally defaults to
            // accelerated compositing. Provider video should never silently fall
            // back to a software-only presentation path just because the page
            // itself does not request acceleration.
            settings.set_hardware_acceleration_policy(
                webkit6::HardwareAccelerationPolicy::Always,
            );
            settings.set_enable_javascript(true);
            settings.set_enable_html5_local_storage(true);
            settings.set_enable_html5_database(true);
            settings.set_enable_media(true);
            settings.set_enable_mediasource(true);
            settings.set_enable_media_capabilities(true);
            settings.set_enable_encrypted_media(true);
            settings.set_enable_fullscreen(true);
            settings.set_enable_webaudio(true);
            settings.set_enable_site_specific_quirks(true);
            settings.set_media_playback_allows_inline(true);
            settings.set_media_playback_requires_user_gesture(false);
            settings.set_javascript_can_open_windows_automatically(true);
            settings.set_enable_write_console_messages_to_stdout(true);
            settings.set_user_agent(Some(ANDROID_WEBVIEW_COMPAT_UA));
        }

        // The Android app uses loadDataWithBaseURL rather than an about:blank
        // document. Give WebKitGTK a real HTTP(S) base URI as well; this helps
        // providers that inspect origin/referrer context for embedded players.
        let base_uri = provider_base_uri(&url);
        webview.load_html(&html, Some(&base_uri));

        // GTK 4.14+ can bypass normal GSK composition and hand suitable DMABUF
        // content directly to the Wayland compositor. Keep this as a hint for
        // integer-scale cases where direct offload is possible. On fractional
        // scaling GTK may reject the subsurface geometry; 0.3.4 defaults GSK
        // to OpenGL on Wayland so that fallback composition still imports
        // WebKit DMABUF frames efficiently instead of bouncing Vulkan -> GL.
        let offload = gtk::GraphicsOffload::new(Some(&webview));
        offload.set_enabled(gtk::GraphicsOffloadEnabled::Enabled);
        offload.set_black_background(true);
        offload.set_hexpand(true);
        offload.set_vexpand(true);

        window.set_child(Some(&offload));
        window.present();
        Ok(())
    }
}

pub fn playback_route(input: &str, iframe: bool) -> Result<PlaybackRoute, PlayerError> {
    let url = checked_url(input)?;
    if iframe || !looks_like_direct_media(&url) {
        Ok(PlaybackRoute::EmbeddedWeb)
    } else {
        Ok(PlaybackRoute::Mpv)
    }
}

const ANDROID_WEBVIEW_COMPAT_UA: &str = "Mozilla/5.0 (Linux; Android 15; Mobile) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36";

fn provider_base_uri(url: &Url) -> String {
    // Preserve an explicit non-default port if the provider uses one.
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{}://{}:{}/", url.scheme(), host, port),
        None => format!("{}://{}/", url.scheme(), host),
    }
}

fn looks_like_direct_media(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    let whole = url.as_str().to_ascii_lowercase();
    const EXTENSIONS: &[&str] = &[
        ".m3u8", ".mpd", ".mp4", ".webm", ".mkv", ".m4v", ".mov", ".ts", ".flv",
    ];
    EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension) || whole.contains(extension))
}

fn checked_url(input: &str) -> Result<Url, PlayerError> {
    let url = Url::parse(input).map_err(|err| PlayerError::InvalidUrl(err.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(PlayerError::UnsupportedScheme);
    }
    Ok(url)
}

pub fn media_check_report() -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "Anixart Arch media diagnostics");
    let _ = writeln!(out, "--------------------------------");
    let _ = writeln!(out, "Session / renderer environment:");
    for name in [
        "XDG_SESSION_TYPE",
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "GDK_BACKEND",
        "GDK_DEBUG",
        "GDK_DISABLE",
        "GSK_RENDERER",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "LIBVA_DRIVER_NAME",
    ] {
        let value = std::env::var(name).unwrap_or_else(|_| "<unset>".to_string());
        let _ = writeln!(out, "  {name}={value}");
    }

    let _ = writeln!(out, "\nGeneral GStreamer media elements:");
    for element in ["hlsdemux", "dashdemux", "avdec_h264", "avdec_hevc", "avdec_aac"] {
        let _ = writeln!(out, "{}", gst_element_status(element));
    }

    let _ = writeln!(out, "\nVA-API hardware video decoders:");
    for element in ["vah264dec", "vah265dec", "vavp9dec", "vaav1dec"] {
        let _ = writeln!(out, "{}", gst_element_status(element));
    }

    let mpv = Command::new("mpv")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if matches!(mpv, Ok(code) if code.success()) {
        let _ = writeln!(out, "\n[OK]      mpv");
    } else {
        let _ = writeln!(out, "\n[MISSING] mpv");
    }

    match Command::new("vainfo").stdin(Stdio::null()).output() {
        Ok(output) if output.status.success() => {
            let _ = writeln!(out, "[OK]      VA-API driver (vainfo)");
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            for line in combined
                .lines()
                .filter(|line| {
                    line.contains("Driver version")
                        || line.contains("VA-API version")
                        || line.contains("vainfo: Supported")
                })
                .take(4)
            {
                let _ = writeln!(out, "          {}", line.trim());
            }
        }
        Ok(_) => {
            let _ = writeln!(out, "[WARN]    vainfo ran but VA-API initialization failed");
        }
        Err(_) => {
            let _ = writeln!(out, "[INFO]    vainfo not installed (optional: libva-utils)");
        }
    }

    let _ = writeln!(out, "\nFullscreen/WebKit diagnostics:");
    let _ = writeln!(out, "  Wayland renderer policy: defaults to GSK_RENDERER=gl when unset");
    let _ = writeln!(out, "  GSK_RENDERER=vulkan anixart      # explicit Vulkan A/B test");
    let _ = writeln!(out, "  GSK_RENDERER=gl anixart          # explicit known-good path");
    let _ = writeln!(out, "  GDK_DEBUG=offload,dmabuf anixart");
    let _ = writeln!(out, "  GST_DEBUG=2 anixart");
    let _ = writeln!(out, "  anixart --webkit-no-dmabuf       # A/B diagnostic, not the default");
    let _ = writeln!(out, "  anixart --webkit-no-compositing  # last-resort diagnostic only");
    out
}

pub fn print_media_check() {
    print!("{}", media_check_report());
}

fn gst_element_status(element: &str) -> String {
    let status = Command::new("gst-inspect-1.0")
        .arg(element)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(code) if code.success() => format!("[OK]      GStreamer {element}"),
        Ok(_) => format!("[MISSING] GStreamer {element}"),
        Err(_) => "[MISSING] gst-inspect-1.0 (install gstreamer)".to_string(),
    }
}

fn escape_html_attr(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_streams_route_to_mpv() {
        assert_eq!(
            playback_route("https://cdn.example/video/master.m3u8?token=abc", false).unwrap(),
            PlaybackRoute::Mpv
        );
        assert_eq!(
            playback_route("https://cdn.example/video.mp4", false).unwrap(),
            PlaybackRoute::Mpv
        );
    }

    #[test]
    fn provider_base_uri_preserves_origin() {
        let url = Url::parse("https://example.test:8443/embed/abc").unwrap();
        assert_eq!(provider_base_uri(&url), "https://example.test:8443/");
    }

    #[test]
    fn iframe_and_provider_pages_route_to_webkit() {
        assert_eq!(
            playback_route("https://provider.example/player/abc", true).unwrap(),
            PlaybackRoute::EmbeddedWeb
        );
        assert_eq!(
            playback_route("https://provider.example/player/abc", false).unwrap(),
            PlaybackRoute::EmbeddedWeb
        );
    }
}
