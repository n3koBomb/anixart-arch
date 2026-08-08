mod api;
mod config;
mod model;
mod player;
mod ui;

use adw::prelude::*;

pub const APP_ID: &str = "io.github.anixartarch.AnixartArch";
pub const APP_NAME: &str = "Anixart";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let renderer_defaulted_to_gl = apply_early_runtime_flags();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if renderer_defaulted_to_gl {
        log::info!(
            "Wayland session detected: defaulting GTK/GSK to the OpenGL renderer for stable WebKit DMABUF composition; set GSK_RENDERER explicitly to override"
        );
    }

    if handle_cli() {
        return;
    }

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(ui::build_ui);
    app.run();
}

fn handle_cli() -> bool {
    let args: Vec<String> = std::env::args()
        .filter(|arg| !matches!(arg.as_str(), "--webkit-no-dmabuf" | "--webkit-no-compositing"))
        .collect();
    if args.len() <= 1 {
        return false;
    }

    match args[1].as_str() {
        "--version" | "-V" => {
            println!("anixart-arch {APP_VERSION}");
            true
        }
        "--help" | "-h" => {
            println!(
                "Anixart {APP_VERSION}\n\n\
                 Usage:\n  anixart\n  anixart --play <https://media-url>\n  anixart --api-check\n  anixart --media-check\n  anixart --webkit-no-dmabuf\n  anixart --webkit-no-compositing\n  anixart --version\n\n\
                 On Wayland, Anixart defaults to GSK_RENDERER=gl unless that\n\
                 environment variable is already set. To retest Vulkan, run\n\
                 GSK_RENDERER=vulkan anixart. The two --webkit-* switches are\n\
                 graphics diagnostics only and continue into the normal GUI.\n"
            );
            true
        }
        "--play" if args.len() >= 3 => {
            if let Err(err) = player::MpvBackend::play(&args[2]) {
                eprintln!("player error: {err}");
                std::process::exit(2);
            }
            true
        }
        "--media-check" => {
            player::print_media_check();
            true
        }
        "--api-check" => {
            let cfg = config::AppConfig::load().unwrap_or_default();
            match api::ApiClient::from_config(&cfg).and_then(|api| api.config_toggles()) {
                Ok(value) => println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
                ),
                Err(err) => {
                    eprintln!("API check failed: {err}");
                    std::process::exit(3);
                }
            }
            true
        }
        _ => false,
    }
}


/// Apply renderer policy and troubleshooting overrides before GTK/WebKit
/// creates any display or web-process state.
///
/// Why OpenGL by default on Wayland?
/// -------------------------------
/// A real-world GTK4/WebKitGTK/KWin trace on fractional scaling (175%) showed
/// that WebKit DMABUF frames using the XR24 modifier were accepted by EGL/GL,
/// while GTK's Vulkan renderer rejected the same modifier and repeatedly fell
/// back through a costly Vulkan -> GL download/conversion path. That path made
/// fullscreen video and even pointer presentation stutter badly.
///
/// The OpenGL renderer imports those DMABUFs directly and remains smooth. We
/// therefore choose it only for Wayland sessions and only when the user did
/// not already set GSK_RENDERER. Explicit user policy always wins.
fn apply_early_runtime_flags() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let mut renderer_defaulted_to_gl = false;

    let wayland_session = std::env::var("XDG_SESSION_TYPE")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some();

    // SAFETY: this is called as the first operation in main(), before GTK,
    // WebKit, the logger, or any application-created thread is initialized.
    // Mutating the process environment is therefore single-threaded here.
    unsafe {
        if wayland_session && std::env::var_os("GSK_RENDERER").is_none() {
            std::env::set_var("GSK_RENDERER", "gl");
            renderer_defaulted_to_gl = true;
        }

        if args.iter().any(|arg| arg == "--webkit-no-dmabuf") {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        if args.iter().any(|arg| arg == "--webkit-no-compositing") {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
    }

    renderer_defaulted_to_gl
}
