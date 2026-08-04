use std::path::Path;

pub struct ServoWebEngine {
    url: String,
}

impl ServoWebEngine {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    pub fn get_url(&self) -> &str {
        &self.url
    }

    pub fn render_preview_file(&self, target_path: &Path) {
        let resolved = crate::config::resolve_asset_path(&self.url);
        crate::webkit_render::start_live_pip(&resolved, target_path);
    }
}
