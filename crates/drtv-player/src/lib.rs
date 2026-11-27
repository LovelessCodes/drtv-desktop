//! libmpv, embedded in an OpenGL framebuffer owned by the app.
//!
//! The player is deliberately small: a [`Player`] trait that names the operations a screen needs,
//! and one backend on top of libmpv's render API. mpv does the decoding, the HLS ABR ladder, the
//! audio clock and the subtitle rendering; this crate's job is to hand it a GL context, an FBO and
//! a rectangle.
//!
//! Why the render API rather than `--wid`: `--wid` hands mpv the window and lets it draw in its own
//! child surface, which cannot be composited under an egui overlay. Rendering into a texture keeps
//! the video inside the same frame as the HUD.

use std::ffi::{CStr, CString, c_void};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::PaintCallbackInfo;
use glow::HasContext as _;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use libmpv2::Mpv;

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("libmpv could not be started: {0}")]
    Mpv(#[from] libmpv2::Error),
    #[error("OpenGL rejected the video framebuffer ({0})")]
    Framebuffer(String),
    #[error("the video surface could not be created: {0}")]
    Gl(String),
}

/// One selectable track. mpv numbers audio, video and subtitle tracks independently, which is why
/// the kind is part of the identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub title: String,
    /// ISO code as the stream spells it (`da`, `en`), not a display name.
    pub lang: String,
    pub selected: bool,
    /// Burned into the stream rather than supplied as a separate file/rendition.
    pub external: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

impl TrackKind {
    fn parse(kind: &str) -> Option<Self> {
        match kind {
            "video" => Some(TrackKind::Video),
            "audio" => Some(TrackKind::Audio),
            "sub" => Some(TrackKind::Subtitle),
            _ => None,
        }
    }

    fn property(self) -> &'static str {
        match self {
            TrackKind::Video => "vid",
            TrackKind::Audio => "aid",
            TrackKind::Subtitle => "sid",
        }
    }
}

/// What happened since the last frame. mpv reports these through its event queue; the UI polls it
/// once per frame rather than running a thread for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A file was opened and playback is about to start.
    Loaded,
    /// The file ended. `eof` is false when it ended because of an error or a stop.
    Ended { eof: bool, error: Option<String> },
    /// The video dimensions changed; the framebuffer will be resized on the next render.
    VideoReconfig,
}

/// The video surface: one texture and the FBO that renders into it.
struct Surface {
    texture: glow::Texture,
    framebuffer: glow::Framebuffer,
    size: (i32, i32),
}

/// The part of the player a screen talks to, so a second backend (a platform-native one, a test
/// double) does not have to look like libmpv.
pub trait Backend {
    fn load(&mut self, url: &str, start_at: Option<f64>);
    fn set_paused(&self, paused: bool);
    fn toggle_pause(&self);
    fn paused(&self) -> bool;
    fn seek_to(&self, seconds: f64);
    fn position(&self) -> Option<f64>;
    fn duration(&self) -> Option<f64>;
    fn set_volume(&self, volume: f64);
    fn volume(&self) -> f64;
    fn set_muted(&self, muted: bool);
    fn muted(&self) -> bool;
    fn set_speed(&self, speed: f64);
    fn tracks(&self) -> Vec<Track>;
    fn select_track(&self, kind: TrackKind, id: i64);
    fn set_subtitles_visible(&self, visible: bool);
    fn stop(&self);
}

/// libmpv.
pub struct VideoPlayer {
    // **Field order matters.** The render context holds GL objects that must be released while the
    // mpv instance is still alive, and Rust drops fields in declaration order.
    render: RenderContext<'static>,
    mpv: Mpv,
    gl: Arc<glow::Context>,
    surface: Option<Surface>,
    /// Set by mpv's update callback, cleared when a frame is drawn. Rendering only on change keeps a
    /// paused video from burning a GPU frame every vsync.
    dirty: Arc<AtomicBool>,
    pending_seek: Option<f64>,
    eof: bool,
    failed: Option<String>,
}

/// The GL context mpv calls back into while it sets itself up. It has to outlive
/// `create_render_context`, which frees it as soon as mpv has copied what it needs.
struct ProcLoader {
    get_proc_address: Arc<dyn Fn(&CStr) -> *const c_void + Send + Sync>,
}

fn proc_address(loader: &ProcLoader, name: &str) -> *mut c_void {
    match CString::new(name) {
        Ok(name) => (loader.get_proc_address)(name.as_c_str()) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

impl VideoPlayer {
    /// Create the player. `repaint` is called from mpv's render thread when a new frame is ready; it
    /// must only wake the UI (egui's `request_repaint`).
    pub fn new(
        gl: Arc<glow::Context>,
        get_proc_address: Arc<dyn Fn(&CStr) -> *const c_void + Send + Sync>,
        repaint: impl Fn() + Send + 'static,
    ) -> Result<Self, PlayerError> {
        let mpv = Mpv::with_initializer(|init| {
            // `vo=libmpv` is what makes mpv hand us frames instead of opening its own window.
            init.set_property("vo", "libmpv")?;
            init.set_property("hwdec", "auto-safe")?;
            // Keep the last frame on screen when playback ends, so the HUD's "next episode" card
            // has something behind it.
            init.set_property("keep-open", "yes")?;
            // The app draws its own controls; mpv's would fight them for the mouse.
            init.set_property("osc", false)?;
            init.set_property("input-default-bindings", "no")?;
            init.set_property("input-vo-keyboard", "no")?;
            // An unconfigured mpv still writes to stdout on some errors; keep the terminal clean.
            init.set_property("terminal", "no")?;
            init.set_property("audio-client-name", "DRTV Desktop")?;
            // A/V sync: with the render API the video is presented by us, so mpv must not add its
            // own display delay on top.
            init.set_property("video-timing-offset", 0.0)?;
            Ok(())
        })?;

        let loader = ProcLoader { get_proc_address };
        let render = mpv.create_render_context([
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address: proc_address,
                ctx: loader,
            }),
        ])?;

        // The context borrows the `Mpv` it came from. Both live in this struct for the whole
        // lifetime of the player and are dropped in the right order (see the field comment), so the
        // borrow is lifted to `'static` here rather than threading a lifetime through every screen.
        let mut render: RenderContext<'static> =
            unsafe { std::mem::transmute::<RenderContext<'_>, RenderContext<'static>>(render) };

        let dirty = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&dirty);
        render.set_update_callback(move || {
            flag.store(true, Ordering::Release);
            repaint();
        });

        Ok(Self {
            render,
            mpv,
            gl,
            surface: None,
            dirty,
            pending_seek: None,
            eof: false,
            failed: None,
        })
    }

    /// The raw mpv handle, for the few things this crate does not wrap (statistics, on-screen text).
    pub fn mpv(&self) -> &Mpv {
        &self.mpv
    }

    /// Draw the current frame into `size` and report whether anything was drawn. The FBO is resized
    /// (and its texture reallocated) when the window changes size.
    pub fn render(&mut self, size: (i32, i32)) -> Result<bool, PlayerError> {
        if size.0 < 2 || size.1 < 2 {
            return Ok(false);
        }
        if self.surface.as_ref().is_none_or(|s| s.size != size) {
            self.resize(size)?;
            self.dirty.store(true, Ordering::Release);
        }
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return Ok(false);
        }
        let Some(surface) = self.surface.as_ref() else {
            return Ok(false);
        };
        // mpv clears the FBO it is given, so the whole target is valid after a draw — no stale
        // border when the aspect ratio letterboxes.
        self.render
            .render::<ProcLoader>(surface.framebuffer.0.get() as i32, size.0, size.1, true)
            .map_err(PlayerError::Mpv)?;
        Ok(true)
    }

    /// The framebuffer holding the last drawn frame. `None` before the first draw.
    pub fn framebuffer(&self) -> Option<(glow::Framebuffer, (i32, i32))> {
        self.surface.as_ref().map(|s| (s.framebuffer, s.size))
    }

    fn resize(&mut self, size: (i32, i32)) -> Result<(), PlayerError> {
        unsafe {
            if let Some(surface) = self.surface.take() {
                self.gl.delete_framebuffer(surface.framebuffer);
                self.gl.delete_texture(surface.texture);
            }
            let texture = self
                .gl
                .create_texture()
                .map_err(|e| PlayerError::Gl(e.to_string()))?;
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                size.0,
                size.1,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.gl.bind_texture(glow::TEXTURE_2D, None);

            let framebuffer = self
                .gl
                .create_framebuffer()
                .map_err(|e| PlayerError::Gl(e.to_string()))?;
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            self.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let status = self.gl.check_framebuffer_status(glow::FRAMEBUFFER);
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            if status != glow::FRAMEBUFFER_COMPLETE {
                self.gl.delete_framebuffer(framebuffer);
                self.gl.delete_texture(texture);
                return Err(PlayerError::Framebuffer(format!("0x{status:x}")));
            }
            self.surface = Some(Surface {
                texture,
                framebuffer,
                size,
            });
        }
        Ok(())
    }

    /// Drain mpv's event queue. Called once per frame.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        // A file can produce a burst of events; the queue is finite and a zero timeout returns
        // immediately when it is empty.
        for _ in 0..64 {
            let Some(event) = self.mpv.wait_event(0.0) else {
                break;
            };
            match event {
                Ok(libmpv2::events::Event::FileLoaded) => {
                    self.eof = false;
                    self.failed = None;
                    events.push(Event::Loaded);
                }
                Ok(libmpv2::events::Event::VideoReconfig) => events.push(Event::VideoReconfig),
                Ok(libmpv2::events::Event::EndFile(_)) => {
                    // The reason enum is a C constant set; the property says the same thing in a
                    // form that needs no binding gymnastics: did the file reach its end, or was it
                    // stopped/errored?
                    let eof = self.mpv.get_property::<bool>("eof-reached").unwrap_or(false);
                    if !eof {
                        self.failed = Some("afspilningen stoppede".to_string());
                    }
                    self.eof = eof;
                    events.push(Event::Ended {
                        eof,
                        error: self.failed.clone().filter(|e| !e.is_empty()),
                    });
                }
                _ => {}
            }
        }
        events
    }

    /// Apply a seek that was asked for before the file had a duration. mpv ignores seeks on a file
    /// it has not opened yet, which is exactly the moment a "resume from 24:00" lands.
    pub fn apply_pending_seek(&mut self) {
        if let Some(seconds) = self.pending_seek
            && self.duration().is_some_and(|d| d > 0.0)
        {
            self.mpv.command("seek", &[&seconds.to_string(), "absolute"]).ok();
            self.pending_seek = None;
        }
    }

    pub fn eof(&self) -> bool {
        self.eof
    }

    pub fn error(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    fn command(&self, name: &str, args: &[&str]) -> bool {
        self.mpv.command(name, args).is_ok()
    }
}

impl Backend for VideoPlayer {
    fn load(&mut self, url: &str, start_at: Option<f64>) {
        self.eof = false;
        self.failed = None;
        self.pending_seek = start_at.filter(|s| *s > 1.0);
        // `replace` is the default but naming it keeps the intent: a new title must not queue
        // behind the previous one.
        if !self.command("loadfile", &[url, "replace"]) {
            self.failed = Some("filed kunne ikke åbnes".to_string());
        }
        // mpv starts paused only if told to; the app always starts playing.
        self.mpv.set_property("pause", false).ok();
    }

    fn set_paused(&self, paused: bool) {
        self.mpv.set_property("pause", paused).ok();
    }

    fn toggle_pause(&self) {
        let paused: bool = self.mpv.get_property("pause").unwrap_or(false);
        self.set_paused(!paused);
    }

    fn paused(&self) -> bool {
        self.mpv.get_property("pause").unwrap_or(false)
    }

    fn seek_to(&self, seconds: f64) {
        self.mpv
            .command("seek", &[&seconds.max(0.0).to_string(), "absolute"])
            .ok();
    }

    fn position(&self) -> Option<f64> {
        self.mpv.get_property::<f64>("time-pos").ok()
    }

    fn duration(&self) -> Option<f64> {
        self.mpv.get_property::<f64>("duration").ok()
    }

    fn set_volume(&self, volume: f64) {
        self.mpv.set_property("volume", volume.clamp(0.0, 130.0)).ok();
    }

    fn volume(&self) -> f64 {
        self.mpv.get_property("volume").unwrap_or(100.0)
    }

    fn set_muted(&self, muted: bool) {
        self.mpv.set_property("mute", muted).ok();
    }

    fn muted(&self) -> bool {
        self.mpv.get_property("mute").unwrap_or(false)
    }

    fn set_speed(&self, speed: f64) {
        self.mpv.set_property("speed", speed.clamp(0.25, 4.0)).ok();
    }

    /// Read the track list one property at a time. mpv exposes `track-list/N/<field>` paths, which
    /// avoids pulling in its node API for a menu with four entries.
    fn tracks(&self) -> Vec<Track> {
        let Ok(count) = self.mpv.get_property::<i64>("track-list/count") else {
            return Vec::new();
        };
        (0..count)
            .filter_map(|i| {
                let kind: String = self.mpv.get_property(&format!("track-list/{i}/type")).ok()?;
                let kind = TrackKind::parse(&kind)?;
                Some(Track {
                    id: self.mpv.get_property(&format!("track-list/{i}/id")).ok()?,
                    kind,
                    title: self.mpv.get_property(&format!("track-list/{i}/title")).unwrap_or_default(),
                    lang: self.mpv.get_property(&format!("track-list/{i}/lang")).unwrap_or_default(),
                    selected: self
                        .mpv
                        .get_property::<bool>(&format!("track-list/{i}/selected"))
                        .unwrap_or(false),
                    external: self
                        .mpv
                        .get_property::<bool>(&format!("track-list/{i}/external"))
                        .unwrap_or(false),
                })
            })
            .collect()
    }

    fn select_track(&self, kind: TrackKind, id: i64) {
        self.mpv.set_property(kind.property(), id).ok();
    }

    fn set_subtitles_visible(&self, visible: bool) {
        // `sid` accepts "no" and "auto"; "auto" picks the stream's default again.
        self.mpv
            .set_property("sid", if visible { "auto" } else { "no" })
            .ok();
    }

    fn stop(&self) {
        self.mpv.command("stop", &[]).ok();
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        // Release the GL objects while the context is still current and mpv still exists.
        if let Some(surface) = self.surface.take() {
            unsafe {
                self.gl.delete_framebuffer(surface.framebuffer);
                self.gl.delete_texture(surface.texture);
            }
        }
    }
}

/// Blit the player's framebuffer onto the egui paint callback's viewport.
///
/// A paint callback gets the window framebuffer with the viewport already set to the widget rect, but
/// mpv sizes its output to the whole FBO it is given — so the video is rendered offscreen and copied
/// here, clipped by the callback's scissor.
pub fn blit(
    gl: &glow::Context,
    info: &PaintCallbackInfo,
    framebuffer: glow::Framebuffer,
    size: (i32, i32),
) {
    unsafe {
        let previous = gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(framebuffer));

        let viewport = info.viewport_in_pixels();
        // GL's framebuffer origin is bottom-left; egui's viewport is measured from the top.
        let screen_height = info.screen_size_px[1] as i32;
        gl.blit_framebuffer(
            0,
            0,
            size.0,
            size.1,
            viewport.left_px,
            screen_height - (viewport.top_px + viewport.height_px),
            viewport.left_px + viewport.width_px,
            screen_height - viewport.top_px,
            glow::COLOR_BUFFER_BIT,
            glow::LINEAR,
        );

        let previous = u32::try_from(previous)
            .ok()
            .and_then(NonZeroU32::new)
            .map(glow::NativeFramebuffer);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, previous);
    }
}

/// A terse "01:23:45"-style clock for the HUD. Hours are dropped under an hour, because a 45-minute
/// episode reading `00:45:00` wastes the two characters that matter.
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_drops_an_empty_hour_and_pads_the_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(9.4), "0:09");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(3600.0), "1:00:00");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(-4.0), "0:00");
    }

    #[test]
    fn track_kinds_map_to_the_property_mpv_selects_with() {
        assert_eq!(TrackKind::Audio.property(), "aid");
        assert_eq!(TrackKind::Subtitle.property(), "sid");
        assert_eq!(TrackKind::parse("sub"), Some(TrackKind::Subtitle));
        assert_eq!(TrackKind::parse("attachment"), None);
    }
}
