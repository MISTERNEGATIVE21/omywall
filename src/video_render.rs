use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub struct MadamiruVideoPlayer {
    pipeline: gst::Pipeline,
    latest_frame: Arc<Mutex<Option<VideoFrame>>>,
}

impl MadamiruVideoPlayer {
    pub fn new(video_path: &Path) -> Result<Self, String> {
        let _ = gst::init();

        let abs_path = std::fs::canonicalize(video_path).unwrap_or_else(|_| video_path.to_path_buf());
        let uri = format!("file://{}", abs_path.to_string_lossy());

        let playbin = gst::ElementFactory::make("playbin")
            .property("uri", &uri)
            .build()
            .map_err(|e| format!("Failed to create playbin element: {}", e))?;

        let appsink = gst_app::AppSink::builder()
            .caps(
                &gst::Caps::builder("video/x-raw")
                    .field("format", "RGBA")
                    .build(),
            )
            .max_buffers(1)
            .drop(true)
            .build();

        let latest_frame = Arc::new(Mutex::new(None));
        let frame_store = latest_frame.clone();

        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Error)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let caps = sample.caps().ok_or(gst::FlowError::Error)?;
                    let structure = caps.structure(0).ok_or(gst::FlowError::Error)?;

                    let width = structure.get::<i32>("width").map_err(|_| gst::FlowError::Error)? as u32;
                    let height = structure.get::<i32>("height").map_err(|_| gst::FlowError::Error)? as u32;

                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    let data = map.as_slice().to_vec();

                    if let Ok(mut lock) = frame_store.lock() {
                        *lock = Some(VideoFrame { width, height, data });
                    }

                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        playbin.set_property("video-sink", appsink.upcast::<gst::Element>());

        let pipeline = playbin
            .downcast::<gst::Pipeline>()
            .map_err(|_| "Failed to downcast playbin to pipeline".to_string())?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("Failed to set pipeline state to Playing: {}", e))?;

        Ok(Self {
            pipeline,
            latest_frame,
        })
    }

    pub fn get_current_frame(&self) -> Option<VideoFrame> {
        self.latest_frame.lock().ok()?.clone()
    }

    pub fn stop(&self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl Drop for MadamiruVideoPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}
