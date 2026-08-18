use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::path::Path;
use webkit2gtk::WebViewExt;

/// Render a URL (or local file) as a wlr-layer-shell background surface using
/// pure Rust GTK3 + WebKit2GTK. Replaces the former `python3` GTK layer-shell
/// runner so no external interpreter is required.
#[allow(dead_code)]
pub fn run(url: &str) -> Result<(), String> {
    run_with_options(url, false, "fullscreen", 1280, 720, None)
}

/// Render a desktop widget overlay with transparent background and edge anchoring.
#[allow(dead_code)]
pub fn run_widget(url: &str, position: &str) -> Result<(), String> {
    run_with_options(url, true, position, 480, 560, None)
}

/// Core runner supporting both fullscreen wallpapers and transparent desktop overlay widgets
/// on wlroots / Hyprland layer-shell.
pub fn run_with_options(
    url: &str,
    is_widget: bool,
    position: &str,
    width: i32,
    height: i32,
    target_monitor: Option<&str>,
) -> Result<(), String> {
    std::env::set_var("GDK_BACKEND", "wayland");
    std::env::set_var("WEBKIT_FORCE_COMPOSITING_MODE", "1");
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");

    if gtk::init().is_err() {
        return Err("WebLayer: gtk::init() failed".into());
    }

    let target_url = resolve_target_url(url);

    let display = gdk::Display::default();
    let num_monitors = display.as_ref().map(|d| d.n_monitors()).unwrap_or(1);

    let monitors_to_spawn: Vec<Option<gdk::Monitor>> = if let Some(disp) = display.as_ref() {
        if let Some(target_name) = target_monitor {
            let mut matched = None;
            for i in 0..num_monitors {
                if let Some(mon) = disp.monitor(i) {
                    if let Some(model) = mon.model() {
                        if model == target_name {
                            matched = Some(mon);
                            break;
                        }
                    }
                }
            }
            vec![matched]
        } else if is_widget {
            vec![disp.monitor(0)]
        } else {
            let mut list = Vec::new();
            for i in 0..num_monitors {
                list.push(disp.monitor(i));
            }
            if list.is_empty() {
                vec![None]
            } else {
                list
            }
        }
    } else {
        vec![None]
    };

    for mon_opt in monitors_to_spawn {
        let window = gtk::Window::new(gtk::WindowType::Toplevel);

        if is_widget {
            if let Some(screen) = gtk::prelude::GtkWindowExt::screen(&window) {
                if let Some(visual) = screen.rgba_visual() {
                    window.set_visual(Some(&visual));
                }
            }
            window.set_app_paintable(true);
        }

        if gtk_layer_shell::is_supported() {
            window.init_layer_shell();

            if let Some(ref mon) = mon_opt {
                window.set_monitor(mon);
            }

            if is_widget {
                window.set_namespace("omywall-widget");
                window.set_layer(Layer::Bottom);
                window.set_keyboard_mode(KeyboardMode::None);
                window.set_exclusive_zone(-1);

                let pos_clean = position.to_lowercase().replace(['-', ' '], "_");
                match pos_clean.as_str() {
                    "top_left" => {
                        window.set_anchor(Edge::Top, true);
                        window.set_anchor(Edge::Left, true);
                        window.set_anchor(Edge::Bottom, false);
                        window.set_anchor(Edge::Right, false);
                        window.set_layer_shell_margin(Edge::Top, 24);
                        window.set_layer_shell_margin(Edge::Left, 24);
                    }
                    "bottom_right" => {
                        window.set_anchor(Edge::Bottom, true);
                        window.set_anchor(Edge::Right, true);
                        window.set_anchor(Edge::Top, false);
                        window.set_anchor(Edge::Left, false);
                        window.set_layer_shell_margin(Edge::Bottom, 24);
                        window.set_layer_shell_margin(Edge::Right, 24);
                    }
                    "bottom_left" => {
                        window.set_anchor(Edge::Bottom, true);
                        window.set_anchor(Edge::Left, true);
                        window.set_anchor(Edge::Top, false);
                        window.set_anchor(Edge::Right, false);
                        window.set_layer_shell_margin(Edge::Bottom, 24);
                        window.set_layer_shell_margin(Edge::Left, 24);
                    }
                    "center_dock" | "center" | "dock" => {
                        window.set_anchor(Edge::Bottom, true);
                        window.set_anchor(Edge::Top, false);
                        window.set_anchor(Edge::Left, false);
                        window.set_anchor(Edge::Right, false);
                        window.set_layer_shell_margin(Edge::Bottom, 32);
                    }
                    _ /* default: top_right */ => {
                        window.set_anchor(Edge::Top, true);
                        window.set_anchor(Edge::Right, true);
                        window.set_anchor(Edge::Bottom, false);
                        window.set_anchor(Edge::Left, false);
                        window.set_layer_shell_margin(Edge::Top, 24);
                        window.set_layer_shell_margin(Edge::Right, 24);
                    }
                }
            } else {
                window.set_namespace("omywall-wallpaper");
                window.set_layer(Layer::Background);
                window.set_anchor(Edge::Top, true);
                window.set_anchor(Edge::Bottom, true);
                window.set_anchor(Edge::Left, true);
                window.set_anchor(Edge::Right, true);
                window.set_exclusive_zone(-1);
                window.set_keyboard_mode(KeyboardMode::None);
            }
        } else {
            // Non-wlr compositor fallback
            if is_widget {
                window.set_default_size(width, height);
                window.set_title("OMYWALL Desktop Widget");
                window.set_decorated(false);
                window.set_keep_below(true);
            } else {
                window.set_default_size(1280, 720);
                window.fullscreen();
                window.set_title("OMYWALL Web Wallpaper");
            }
        }

        let settings = webkit2gtk::Settings::builder()
            .enable_webgl(true)
            .enable_media_stream(true)
            .enable_mediasource(true)
            .enable_webaudio(true)
            .media_playback_requires_user_gesture(false)
            .allow_file_access_from_file_urls(true)
            .allow_universal_access_from_file_urls(true)
            .enable_html5_local_storage(true)
            .enable_javascript(true)
            .enable_smooth_scrolling(false)
            .hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Always)
            .build();

        let webview = webkit2gtk::WebView::builder().settings(&settings).build();

        if is_widget {
            let transparent = gdk::RGBA::new(0.0, 0.0, 0.0, 0.0);
            webview.set_background_color(&transparent);
        }

        webview.connect_load_failed(|_webview, _event, _failing_uri, _error| {
            true
        });

        webview.load_uri(&target_url);
        window.add(&webview);
        window.show_all();
        window.present();
    }

    gtk::main();
    Ok(())
}

pub fn resolve_target_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains("youtube.com/watch?v=") || trimmed.contains("youtu.be/") {
        if let Some(id_start) = trimmed.find("v=") {
            let id = &trimmed[id_start + 2..];
            let id = id.split('&').next().unwrap_or(id);
            return format!("https://www.youtube.com/embed/{}?autoplay=1&mute=1&loop=1&playlist={}", id, id);
        } else if let Some(id_start) = trimmed.find("youtu.be/") {
            let id = &trimmed[id_start + 9..];
            let id = id.split('?').next().unwrap_or(id);
            return format!("https://www.youtube.com/embed/{}?autoplay=1&mute=1&loop=1&playlist={}", id, id);
        }
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
        || trimmed.starts_with("data:")
    {
        return trimmed.to_string();
    }

    let resolved = crate::config::resolve_asset_path(trimmed);
    let path = Path::new(&resolved);
    if path.is_dir() {
        if let Some(html_entry) = crate::config::find_primary_html_entry(path) {
            return format!("file://{}", html_entry.display());
        }
    }
    if path.exists() {
        format!("file://{}", resolved)
    } else if trimmed.contains('.') && !trimmed.contains(' ') {
        format!("https://{}", trimmed)
    } else {
        format!("file://{}", resolved)
    }
}
