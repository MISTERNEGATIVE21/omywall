use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk::prelude::*;
use webkit2gtk::WebViewExt;

pub enum RenderCmd {
    Thumbnail { url: String, out: PathBuf },
    Live { url: String, out: PathBuf },
    LiveStop,
    Shutdown,
}

#[derive(Clone)]
struct LiveTarget {
    out: PathBuf,
    last_shot: Option<std::time::Instant>,
    ready_since: Option<std::time::Instant>,
}

struct State {
    thumb_queue: VecDeque<(String, PathBuf)>,
    current_url: Option<String>,
    live: Option<LiveTarget>,
}

pub struct WebkitRenderer {
    tx: Sender<RenderCmd>,
}

static GLOBAL_RENDERER: std::sync::Mutex<Option<Arc<WebkitRenderer>>> = std::sync::Mutex::new(None);
static RENDER_THREAD: std::sync::Mutex<Option<std::thread::JoinHandle<()>>> = std::sync::Mutex::new(None);

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

        match thread {
            Err(_) => return None,
            Ok(handle) => {
                if let Ok(mut t) = RENDER_THREAD.lock() {
                    *t = Some(handle);
                }
            }
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

    pub fn start_live(&self, url: &str, out: &Path) {
        let _ = self.tx.send(RenderCmd::Live {
            url: to_uri(url),
            out: out.to_path_buf(),
        });
    }

    pub fn stop_live(&self) {
        let _ = self.tx.send(RenderCmd::LiveStop);
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(RenderCmd::Shutdown);
    }
}

pub fn start_live_pip(url: &str, out: &Path) {
    if let Some(r) = global_renderer() {
        r.start_live(url, out);
    }
}

pub fn stop_live_pip() {
    if let Some(r) = global_renderer() {
        r.stop_live();
    }
}

pub fn shutdown_global_renderer() {
    let mut guard = match GLOBAL_RENDERER.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(r) = guard.take() {
        r.shutdown();
    }
    drop(guard);
    if let Ok(mut t) = RENDER_THREAD.lock() {
        if let Some(handle) = t.take() {
            let _ = handle.join();
        }
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
    if let Some(live) = s.live.as_mut() {
        live.ready_since = None;
    }
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
    window.set_size_request(640, 360);
    window.set_opacity(0.0);
    window.move_(-10000, -10000);
    window.set_skip_taskbar_hint(true);
    window.set_skip_pager_hint(true);
    window.set_accept_focus(false);

    let webview = webkit2gtk::WebView::builder().settings(&settings).build();
    window.add(&webview);
    window.show_all();

    let state = Arc::new(Mutex::new(State {
        thumb_queue: VecDeque::new(),
        current_url: None,
        live: None,
    }));

    let st = state.clone();
    let wv = webview.clone();
    webview.connect_load_changed(move |_webview, event| {
        if event != webkit2gtk::LoadEvent::Finished {
            return;
        }
        let mut guard = st.lock().unwrap();
        if let Some(live) = guard.live.as_mut() {
            live.ready_since = Some(std::time::Instant::now());
        }
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
                        s.live = None;
                        if s.thumb_queue.len() > 64 || s.thumb_queue.iter().any(|(u, o)| u == &url && o == &out) {
                            false
                        } else {
                            let was_empty = s.thumb_queue.is_empty();
                            s.thumb_queue.push_back((url.clone(), out));
                            was_empty
                        }
                    };
                    if should_load {
                        load_url(&webview, &state, &url);
                    }
                }
                RenderCmd::Live { url, out } => {
                    let changed = {
                        let mut s = state.lock().unwrap();
                        s.thumb_queue.clear();
                        s.live = Some(LiveTarget {
                            out,
                            last_shot: None,
                            ready_since: None,
                        });
                        s.current_url.as_deref() != Some(url.as_str())
                    };
                    if changed {
                        load_url(&webview, &state, &url);
                    }
                }
                RenderCmd::LiveStop => {
                    let next_url = {
                        let mut s = state.lock().unwrap();
                        s.live = None;
                        s.thumb_queue.front().map(|(u, _)| u.clone())
                    };
                    if let Some(url) = next_url {
                        load_url(&webview, &state, &url);
                    }
                }
                RenderCmd::Shutdown => {
                    drop(window);
                    return;
                }
            }
        }

        {
            let shot = {
                let mut s = state.lock().unwrap();
                if let Some(live) = s.live.as_mut() {
                    let now = std::time::Instant::now();
                    let page_ready = live
                        .ready_since
                        .map(|t| now.duration_since(t) >= Duration::from_millis(400))
                        .unwrap_or(false);
                    let due = page_ready
                        && live
                            .last_shot
                            .map(|t| now.duration_since(t) >= Duration::from_millis(200))
                            .unwrap_or(true);
                    if due {
                        live.last_shot = Some(now);
                        Some(live.out.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(out) = shot {
                if capture_widget(&webview, &out) {
                    crate::gui::notify_thumb_updated(out);
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
        if s.thumb_queue.is_empty() || s.live.is_some() {
            return;
        }
    }

    if capture_widget(wv, &target) {
        crate::gui::notify_thumb_updated(target.clone());
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

    static WEBKIT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static WEBKIT_TESTS_REMAINING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(2);

    /// GTK may only be initialized once per process, so the shared renderer
    /// thread must be torn down only after the very last webkit test finishes.
    fn release_shared_renderer() {
        if WEBKIT_TESTS_REMAINING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            shutdown_global_renderer();
        }
    }

    fn shared() -> Arc<WebkitRenderer> {
        assert!(init_global_renderer(), "in-process WebKit renderer should initialize");
        global_renderer().expect("global renderer available")
    }

    fn wait_for_png(path: &Path) -> bool {
        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(150));
            if path.exists() && path.metadata().map(|m| m.len()).unwrap_or(0) > 0 {
                if let Ok(img) = image::open(path) {
                    let img = img.to_rgba8();
                    let non_black = img.pixels().filter(|p| p.0[0] > 40 || p.0[1] > 40 || p.0[2] > 40).count();
                    if non_black > 50 {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn open_image_retry(path: &Path) -> image::DynamicImage {
        for _ in 0..10 {
            if let Ok(img) = image::open(path) {
                return img;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        image::open(path).expect("valid PNG after retry")
    }

    #[test]
    fn renders_html_thumbnail_in_process() {
        let _guard = WEBKIT_TEST_LOCK.lock();
        let renderer = shared();
        let out = std::env::temp_dir().join("omywall_webkit_test.png");
        let _ = std::fs::remove_file(&out);

        renderer.render_thumbnail(
            "data:text/html,<html><body style='background:rgb(10,120,200)'><h1 style='color:white'>OMYWALL</h1></body></html>",
            &out,
        );

        assert!(wait_for_png(&out), "thumbnail PNG should be written by the in-process renderer");
        let img = open_image_retry(&out).to_rgba8();
        assert!(img.width() > 0 && img.height() > 0);
        let non_black = img.pixels().filter(|p| p.0[0] > 40 || p.0[1] > 40 || p.0[2] > 40).count();
        assert!(non_black > 100, "offscreen capture should contain content, got {} non-black px", non_black);
        let _ = std::fs::remove_file(&out);
        release_shared_renderer();
    }

    #[test]
    fn animating_canvas_renders_live_frames() {
        let _guard = WEBKIT_TEST_LOCK.lock();
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
        let _ = std::fs::remove_file(&png1);

        renderer.start_live(page, &png1);

        assert!(wait_for_png(&png1), "live animation frame 1 should render");
        let img1 = open_image_retry(&png1).to_rgba8();

        // Wait for live loop (runs every 200ms) to capture updated animation frames
        std::thread::sleep(Duration::from_millis(600));

        let mut img2 = open_image_retry(&png1).to_rgba8();
        let non_black = |img: &image::RgbaImage| img.pixels().filter(|p| p.0[0] > 40 || p.0[1] > 40 || p.0[2] > 40).count();
        for _ in 0..20 {
            if non_black(&img2) > 100 {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
            img2 = open_image_retry(&png1).to_rgba8();
        }
        renderer.stop_live();

        assert!(non_black(&img1) > 100, "frame 1 should contain visible content");
        assert!(non_black(&img2) > 100, "frame 2 should contain visible content");

        let diff = img1.pixels().zip(img2.pixels()).filter(|(a, b)| a.0 != b.0).count();
        assert!(diff > 50, "frames should differ (animation running), diff={}", diff);

        let _ = std::fs::remove_file(&png1);
        release_shared_renderer();
    }
}
