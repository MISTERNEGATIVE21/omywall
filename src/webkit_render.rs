use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk::prelude::*;
use webkit2gtk::WebViewExt;

pub enum RenderCmd {
    Thumbnail { url: String, out: PathBuf },
}

struct State {
    thumb_queue: VecDeque<(String, PathBuf)>,
    current_url: Option<String>,
}

pub struct WebkitRenderer {
    tx: Sender<RenderCmd>,
}

static GLOBAL_RENDERER: std::sync::Mutex<Option<Arc<WebkitRenderer>>> = std::sync::Mutex::new(None);

pub fn init_global_renderer() -> bool {
    let mut guard = match GLOBAL_RENDERER.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if guard.is_none() {
        *guard = WebkitRenderer::new().map(Arc::new);
    }
    guard.is_some()
}

pub fn global_renderer() -> Option<Arc<WebkitRenderer>> {
    GLOBAL_RENDERER.lock().ok().and_then(|g| g.clone())
}

impl WebkitRenderer {
    pub fn new() -> Option<Self> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        let thread = std::thread::Builder::new()
            .name("omywall-webkit-render".into())
            .spawn(move || render_thread(rx, ready_tx));

        if thread.is_err() {
            return None;
        }

        match ready_rx.recv_timeout(Duration::from_secs(4)) {
            Ok(Ok(())) => Some(Self { tx }),
            _ => None,
        }
    }

    pub fn render_thumbnail(&self, url: &str, out: &Path) {
        let _ = self.tx.send(RenderCmd::Thumbnail {
            url: to_uri(url),
            out: out.to_path_buf(),
        });
    }
}

fn to_uri(url: &str) -> String {
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("file://")
        || url.starts_with("about:")
        || url.starts_with("data:")
    {
        url.to_string()
    } else {
        format!("file://{}", url)
    }
}

fn load_url(wv: &webkit2gtk::WebView, st: &Mutex<State>, url: &str) {
    let mut s = st.lock().unwrap();
    if s.current_url.as_deref() == Some(url) {
        wv.reload();
    } else {
        s.current_url = Some(url.to_string());
        wv.load_uri(url);
    }
}

fn render_thread(rx: Receiver<RenderCmd>, ready: Sender<Result<(), String>>) {
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    if gtk::init().is_err() {
        let _ = ready.send(Err("gtk::init() failed".into()));
        return;
    }

    let settings = webkit2gtk::Settings::builder()
        .enable_webgl(true)
        .enable_media_stream(true)
        .enable_mediasource(true)
        .media_playback_requires_user_gesture(false)
        .allow_file_access_from_file_urls(true)
        .enable_html5_local_storage(true)
        .build();

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_decorated(false);
    window.set_default_size(640, 360);
    window.set_opacity(0.0);
    window.move_(-10000, -10000);

    let webview = webkit2gtk::WebView::builder().settings(&settings).build();
    window.add(&webview);
    window.show_all();

    let state = Arc::new(Mutex::new(State {
        thumb_queue: VecDeque::new(),
        current_url: None,
    }));

    let st = state.clone();
    let wv = webview.clone();
    webview.connect_load_changed(move |_webview, event| {
        if event != webkit2gtk::LoadEvent::Finished {
            return;
        }
        let guard = st.lock().unwrap();
        if let Some((_url, out)) = guard.thumb_queue.front() {
            let out = out.clone();
            let st2 = st.clone();
            let wv2 = wv.clone();
            glib::timeout_add_local(Duration::from_millis(700), move || {
                request_snapshot(&wv2, &st2, out.clone());
                glib::ControlFlow::Break
            });
        }
    });

    let _ = ready.send(Ok(()));

    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                RenderCmd::Thumbnail { url, out } => {
                    let should_load = {
                        let mut s = state.lock().unwrap();
                        let was_empty = s.thumb_queue.is_empty();
                        s.thumb_queue.push_back((url.clone(), out));
                        was_empty
                    };
                    if should_load {
                        load_url(&webview, &state, &url);
                    }
                }
            }
        }

        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }

        std::thread::sleep(Duration::from_millis(8));
    }
}

fn capture_widget(wv: &webkit2gtk::WebView, target: &Path) -> bool {
    let widget: gtk::Widget = wv.clone().upcast();
    if widget.window().is_none() {
        return false;
    }
    let alloc = widget.allocation();
    let w = alloc.width();
    let h = alloc.height();
    if w < 2 || h < 2 {
        return false;
    }
    let surface = match cairo::ImageSurface::create(cairo::Format::ARgb32, w, h) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let cr = match cairo::Context::new(&surface) {
        Ok(c) => c,
        Err(_) => return false,
    };
    widget.draw(&cr);
    let mut file = match std::fs::File::create(target) {
        Ok(f) => f,
        Err(_) => return false,
    };
    surface.write_to_png(&mut file).is_ok()
}

fn request_snapshot(wv: &webkit2gtk::WebView, st: &Arc<Mutex<State>>, target: PathBuf) {
    {
        let s = st.lock().unwrap();
        if s.thumb_queue.is_empty() {
            return;
        }
    }

    if capture_widget(wv, &target) {
        crate::gui::notify_thumb_updated(target.clone());
        if let Some(ctx) = crate::gui::global_egui_ctx() {
            ctx.request_repaint();
        }
    }

    let next_url = {
        let mut s = st.lock().unwrap();
        s.thumb_queue.pop_front();
        s.thumb_queue.front().map(|(u, _)| u.clone())
    };

    if let Some(url) = next_url {
        load_url(wv, st, &url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> Arc<WebkitRenderer> {
        assert!(init_global_renderer(), "in-process WebKit renderer should initialize");
        global_renderer().expect("global renderer available")
    }

    fn wait_for_png(path: &Path) -> bool {
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(200));
            if path.exists() && path.metadata().map(|m| m.len()).unwrap_or(0) > 0 {
                return true;
            }
        }
        false
    }

    #[test]
    fn renders_html_thumbnail_in_process() {
        let renderer = shared();
        let out = std::env::temp_dir().join("omywall_webkit_test.png");
        let _ = std::fs::remove_file(&out);

        renderer.render_thumbnail(
            "data:text/html,<html><body style='background:rgb(10,120,200)'><h1 style='color:white'>OMYWALL</h1></body></html>",
            &out,
        );

        assert!(wait_for_png(&out), "thumbnail PNG should be written by the in-process renderer");
        let img = image::open(&out).expect("valid PNG");
        assert!(img.width() > 0 && img.height() > 0);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn animating_canvas_renders_live_frames() {
        let renderer = shared();

        let page = r#"data:text/html,<html><body style='margin:0;background:rgb(20,20,30)'><canvas id='c' width='320' height='180'></canvas>
        <script>
        const cv=document.getElementById('c'),x=cv.getContext('2d');let t=0;
        function frame(){
          t+=0.05;
          x.fillStyle='rgb(20,20,30)';x.fillRect(0,0,320,180);
          x.fillStyle='rgb(255,80,40)';
          const px=Math.cos(t)*120+160, py=Math.sin(t)*120+90;
          x.fillRect(px,py,40,40);
          requestAnimationFrame(frame);
        }
        frame();
        </script></body></html>"#;

        let png1 = std::env::temp_dir().join("omywall_anim_test_1.png");
        let png2 = std::env::temp_dir().join("omywall_anim_test_2.png");
        let _ = std::fs::remove_file(&png1);
        let _ = std::fs::remove_file(&png2);

        renderer.render_thumbnail(page, &png1);
        std::thread::sleep(Duration::from_millis(800));
        renderer.render_thumbnail(page, &png2);
        std::thread::sleep(Duration::from_millis(800));

        assert!(wait_for_png(&png1) && wait_for_png(&png2), "both animation frames should render");

        let img1 = image::open(&png1).unwrap().to_rgba8();
        let img2 = image::open(&png2).unwrap().to_rgba8();

        let non_black = |img: &image::RgbaImage| img.pixels().filter(|p| p.0[0] > 40 || p.0[1] > 40 || p.0[2] > 40).count();
        assert!(non_black(&img1) > 100, "frame 1 should contain visible content");
        assert!(non_black(&img2) > 100, "frame 2 should contain visible content");

        let diff = img1.pixels().zip(img2.pixels()).filter(|(a, b)| a.0 != b.0).count();
        assert!(diff > 50, "frames should differ (animation running), diff={}", diff);

        let _ = std::fs::remove_file(&png1);
        let _ = std::fs::remove_file(&png2);
    }
}
