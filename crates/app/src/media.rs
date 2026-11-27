//! Whatever is playing on the PC (Spotify, YouTube in a browser, Netflix, VLC, Media Player…),
//! read from Windows' own media sessions: the same source as the media box Windows shows when you
//! press a volume key. Windows reports every change (another song, play / pause, a jump in the
//! track) as it happens; between reports the position is worked out from the moment the app last
//! gave it, so the progress bar glides and stays in step with the player.
use crate::winrt::wait;
use ronin_core::media as m;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, TimelinePropertiesChangedEventArgs,
};

/// Album art or a video's picture, as RGBA pixels (width, height, pixels).
pub type Art = Arc<(usize, usize, Vec<u8>)>;

#[derive(Clone, Debug, Default)]
pub struct Media {
    pub title: String,
    pub artist: String,
    /// A readable name for the app playing it ("Spotify", "Brave").
    pub app: String,
    pub playing: bool,
    /// Where playback was at `at`, and how long the track is (zero when unknown, e.g. a live stream).
    pub position: Duration,
    pub at: Option<Instant>,
    pub end: Duration,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_seek: bool,
    pub art: Option<Art>,
    /// Changes whenever the picture does, so the bar uploads it only once.
    pub art_id: u64,
}

impl Media {
    /// Where playback is right now (moves on by itself while playing).
    pub fn position(&self) -> Duration {
        let since = self.at.map(|a| a.elapsed()).unwrap_or_default();
        m::position_now(self.position, since, self.playing, self.end)
    }
}

pub enum MediaCmd { Refresh, Details, Toggle, Next, Prev, Seek(Duration) }

pub type Shared = Arc<Mutex<Option<Media>>>;

pub fn start(wake: eframe::egui::Context) -> (Shared, Sender<MediaCmd>) {
    let state: Shared = Arc::new(Mutex::new(None));
    let (tx, rx) = channel();
    let (s, again) = (state.clone(), tx.clone());
    std::thread::spawn(move || run(s, rx, again, wake));
    (state, tx)
}

fn now_ticks() -> i64 {
    m::ticks_from_unix(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default())
}

fn span(t: windows::Foundation::TimeSpan) -> Duration { Duration::from_nanos(t.Duration.max(0) as u64 * 100) }

/// The picture of what is playing, made small (it is shown at most a few dozen points wide).
fn read_art(props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<Art> {
    use windows::Storage::Streams::DataReader;
    let stream = wait(props.Thumbnail().ok()?.OpenReadAsync(), Duration::from_secs(3)).ok()?;
    let size = stream.Size().ok()?.min(8 * 1024 * 1024) as u32;
    if size == 0 { return None }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let load = reader.LoadAsync(size).ok()?.cast::<windows_future::IAsyncOperation<u32>>().ok()?;
    let n = wait(Ok(load), Duration::from_secs(3)).ok()? as usize;
    let mut buf = vec![0u8; n];
    reader.ReadBytes(&mut buf).ok()?;
    let img = image::load_from_memory(&buf).ok()?.thumbnail(200, 200).into_rgba8();
    Some(Arc::new((img.width() as usize, img.height() as usize, img.into_raw())))
}

struct Watched {
    session: Session,
    tokens: (i64, i64, i64),
}

impl Drop for Watched {
    fn drop(&mut self) {
        let _ = self.session.RemoveMediaPropertiesChanged(self.tokens.0);
        let _ = self.session.RemovePlaybackInfoChanged(self.tokens.1);
        let _ = self.session.RemoveTimelinePropertiesChanged(self.tokens.2);
    }
}

fn watch(session: Session, tx: &Sender<MediaCmd>) -> Watched {
    let (a, b, c) = (tx.clone(), tx.clone(), tx.clone());
    let t0 = session.MediaPropertiesChanged(&TypedEventHandler::<Session, MediaPropertiesChangedEventArgs>::new(move |_, _| { let _ = a.send(MediaCmd::Details); Ok(()) })).unwrap_or(0);
    let t1 = session.PlaybackInfoChanged(&TypedEventHandler::<Session, PlaybackInfoChangedEventArgs>::new(move |_, _| { let _ = b.send(MediaCmd::Refresh); Ok(()) })).unwrap_or(0);
    let t2 = session.TimelinePropertiesChanged(&TypedEventHandler::<Session, TimelinePropertiesChangedEventArgs>::new(move |_, _| { let _ = c.send(MediaCmd::Refresh); Ok(()) })).unwrap_or(0);
    Watched { session, tokens: (t0, t1, t2) }
}

fn run(state: Shared, rx: Receiver<MediaCmd>, tx: Sender<MediaCmd>, wake: eframe::egui::Context) {
    crate::sys::com_init_mta();
    let Ok(manager) = wait(Manager::RequestAsync(), Duration::from_secs(8)) else { return };
    let t = tx.clone();
    let _ = manager.CurrentSessionChanged(&TypedEventHandler::<Manager, CurrentSessionChangedEventArgs>::new(move |_, _| { let _ = t.send(MediaCmd::Details); Ok(()) }));
    let set = |f: &dyn Fn(&mut Option<Media>)| { if let Ok(mut s) = state.lock() { f(&mut s); } wake.request_repaint(); };
    let mut watched: Option<Watched> = None;
    let mut art_id = 0u64;
    let mut first = true;
    loop {
        // Windows' reports drive everything; a quiet look every couple of seconds is only a safety net.
        let cmd = if first { first = false; Some(MediaCmd::Details) } else {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(c) => Some(c),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        };
        // Reports often come in bursts: one look covers them all.
        let mut details = matches!(cmd, Some(MediaCmd::Details));
        let mut action = match cmd { Some(MediaCmd::Toggle) | Some(MediaCmd::Next) | Some(MediaCmd::Prev) | Some(MediaCmd::Seek(_)) => cmd, _ => None };
        while let Ok(more) = rx.try_recv() {
            match more {
                MediaCmd::Details => details = true,
                MediaCmd::Refresh => {}
                other => action = Some(other),
            }
        }
        // Follow whichever session Windows calls current (the app you used last).
        let current = manager.GetCurrentSession().ok();
        let same = match (&watched, &current) {
            (Some(w), Some(c)) => w.session.as_raw() == c.as_raw() || w.session.SourceAppUserModelId().ok() == c.SourceAppUserModelId().ok(),
            (None, None) => true,
            _ => false,
        };
        if !same {
            watched = current.clone().map(|s| watch(s, &tx));
            details = true;
        }
        let Some(session) = current else { set(&|s| *s = None); continue };
        if let Some(a) = action {
            let r = match a {
                MediaCmd::Toggle => session.TryTogglePlayPauseAsync(),
                MediaCmd::Next => session.TrySkipNextAsync(),
                MediaCmd::Prev => session.TrySkipPreviousAsync(),
                MediaCmd::Seek(d) => session.TryChangePlaybackPositionAsync((d.as_nanos() / 100) as i64),
                _ => unreachable!(),
            };
            let _ = wait(r, Duration::from_secs(3));
        }
        // Play state, buttons and the timeline: cheap, read every time.
        let info = session.GetPlaybackInfo().ok();
        let playing = info.as_ref().and_then(|i| i.PlaybackStatus().ok()) == Some(Status::Playing);
        let ctl = info.as_ref().and_then(|i| i.Controls().ok());
        let (can_prev, can_next, can_seek) = ctl.map(|c| (c.IsPreviousEnabled().unwrap_or(false), c.IsNextEnabled().unwrap_or(false), c.IsPlaybackPositionEnabled().unwrap_or(false))).unwrap_or((false, false, false));
        let tl = session.GetTimelineProperties().ok();
        let (mut position, mut end) = (Duration::ZERO, Duration::ZERO);
        if let Some(tl) = &tl {
            let reported = tl.Position().map(span).unwrap_or_default();
            end = tl.EndTime().map(span).unwrap_or_default();
            // The app said `reported` at `LastUpdatedTime`: carry it forward to this moment.
            let since = tl.LastUpdatedTime().map(|d| m::ago(d.UniversalTime, now_ticks())).unwrap_or_default();
            position = m::position_now(reported, since.min(Duration::from_secs(6 * 3600)), playing, end);
        }
        let app = m::app_name(&session.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default());
        // Title, artist and picture: only when they change.
        let mut fresh: Option<(String, String, Option<Art>)> = None;
        if details {
            if let Ok(props) = wait(session.TryGetMediaPropertiesAsync(), Duration::from_secs(3)) {
                let title = props.Title().map(|s| s.to_string()).unwrap_or_default();
                let artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
                fresh = Some((title, artist, read_art(&props)));
                art_id += 1;
            }
        }
        let now = Instant::now();
        set(&|s| {
            let mut md = s.take().unwrap_or_default();
            if let Some((title, artist, art)) = &fresh {
                md.title = title.chars().filter(|c| !c.is_control()).take(200).collect();
                md.artist = artist.chars().filter(|c| !c.is_control()).take(200).collect();
                md.art = art.clone();
                md.art_id = art_id;
            }
            md.app = app.clone();
            md.playing = playing;
            md.position = position;
            md.at = Some(now);
            md.end = end;
            md.can_prev = can_prev;
            md.can_next = can_next;
            md.can_seek = can_seek;
            // Nothing to show for a session without a title (an app that only registered itself).
            *s = (!md.title.is_empty()).then_some(md);
        });
    }
}
