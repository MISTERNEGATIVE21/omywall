use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::path::Path;
use webkit2gtk::WebViewExt;

/// Render a URL (or local file) as a wlr-layer-shell background surface using
/// pure Rust GTK3 + WebKit2GTK. Replaces the former `python3` GTK layer-shell
/// runner so no external interpreter is required.
pub fn run(url: &str) -> Result<(), String> {
    std::env::set_var("GDK_BACKEND", "wayland");
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");

    if gtk::init().is_err() {
        return Err("WebLayer: gtk::init() failed".into());
    }



    let target_url = resolve_target_url(url);

    let window = gtk::Window::new(gtk::WindowType::Toplevel);

    if gtk_layer_shell::is_supported() {
        window.init_layer_shell();
        window.set_layer(Layer::Background);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Bottom, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Right, true);
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(KeyboardMode::None);
    } else {
        // Non-wlr compositor fallback: a plain fullscreen window.
        window.set_default_size(1280, 720);
        window.fullscreen();
        window.set_title("OMYWALL Web Wallpaper");
    }

    let settings = webkit2gtk::Settings::builder()
        .enable_webgl(true)
        .enable_media_stream(true)
        .enable_mediasource(true)
        .media_playback_requires_user_gesture(false)
        .allow_file_access_from_file_urls(true)
        .enable_html5_local_storage(true)
        .build();

    let webview = webkit2gtk::WebView::builder().settings(&settings).build();

    webview.connect_load_failed(|_webview, _event, _failing_uri, _error| {
        true
    });

    webview.load_uri(&target_url);
    window.add(&webview);
    window.show_all();
    window.present();

    gtk::main();
    Ok(())
}

fn resolve_target_url(raw: &str) -> String {
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
    if Path::new(&resolved).exists() {
        format!("file://{}", resolved)
    } else if trimmed.contains('.') && !trimmed.contains(' ') {
        format!("https://{}", trimmed)
    } else {
        format!("file://{}", resolved)
    }
}

