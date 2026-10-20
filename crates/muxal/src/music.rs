//! One lazy local player per app. All discovery/device/decoder work stays off GPUI.
use crate::i18n::t;
use muxal_core::music::{MusicSettings, Queue, supported};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Loading,
    Paused,
    Playing,
    Empty,
    Error,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub track: Option<PathBuf>,
    pub count: usize,
    pub partial: bool,
    pub error: Option<String>,
}
#[derive(Clone, Copy)]
pub enum Command {
    Play,
    Pause,
    Skip,
    Previous,
    Volume(u8),
    Stop,
}

pub struct Service {
    sender: mpsc::SyncSender<Command>,
    cancelled: Arc<AtomicBool>,
    repeat_track: Arc<AtomicBool>,
}
impl Service {
    pub fn start(
        settings: MusicSettings,
    ) -> std::io::Result<(Self, async_channel::Receiver<Status>)> {
        if !settings.enabled || settings.folder.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Music is disabled or has no folder",
            ));
        }
        let (sender, receiver) = mpsc::sync_channel(32);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let repeat_track = Arc::new(AtomicBool::new(settings.repeat_track));
        let repeat = repeat_track.clone();
        let (events, latest) = async_channel::bounded(1);
        let updates = Updates {
            events,
            latest: latest.clone(),
        };
        std::thread::Builder::new()
            .name("muxal-music".into())
            .spawn(move || {
                if !settings.enabled {
                    return;
                }
                let folder = settings.folder.clone().unwrap_or_default();
                let discovery = discover(&folder, &stop);
                match discovery {
                    Ok((tracks, partial)) if !stop.load(Ordering::Acquire) => {
                        let queue = Queue::new(tracks, settings.last_track.as_deref());
                        run(
                            queue,
                            partial,
                            settings.volume,
                            &folder,
                            receiver,
                            &stop,
                            &repeat,
                            &updates,
                            NativeAudio::default(),
                        );
                    }
                    Err(error) if !stop.load(Ordering::Acquire) => {
                        updates.send(Status {
                            state: State::Error,
                            error: Some(error),
                            ..Status::default()
                        });
                    }
                    _ => {}
                }
            })?;
        Ok((
            Self {
                sender,
                cancelled,
                repeat_track,
            },
            latest,
        ))
    }
    pub fn send(&self, command: Command) {
        let _ = self.sender.try_send(command);
    }
    pub fn set_repeat_track(&self, enabled: bool) {
        self.repeat_track.store(enabled, Ordering::Release);
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        // Also checked by the source on the audio callback: queued commands or
        // late device/decoder creation cannot keep disabled music audible.
        self.cancelled.store(true, Ordering::Release);
        self.send(Command::Stop);
    }
}

struct Updates {
    events: async_channel::Sender<Status>,
    latest: async_channel::Receiver<Status>,
}
impl Updates {
    fn send(&self, status: Status) {
        if let Err(async_channel::TrySendError::Full(status)) = self.events.try_send(status) {
            let _ = self.latest.try_recv();
            let _ = self.events.try_send(status);
        }
    }
}

fn discover(folder: &Path, cancelled: &AtomicBool) -> Result<(Vec<PathBuf>, bool), String> {
    const TRACKS: usize = 10_000;
    const ENTRIES: usize = 50_000;
    if !folder.is_dir() {
        return Err(t("Music folder is unavailable").to_string());
    }
    let mut paths = Vec::new();
    let started = Instant::now();
    let mut partial = false;
    // Unlike project discovery, music ignores neither tracked state nor gitignore.
    let walk = ignore::WalkBuilder::new(folder)
        .hidden(true)
        .follow_links(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        .max_depth(Some(16))
        .build();
    for (visited, entry) in walk.enumerate() {
        if cancelled.load(Ordering::Acquire) {
            return Ok((Vec::new(), false));
        }
        if visited >= ENTRIES
            || paths.len() >= TRACKS
            || started.elapsed() > Duration::from_secs(10)
        {
            partial = true;
            break;
        }
        match entry {
            Ok(entry)
                if entry.file_type().is_some_and(|t| t.is_file()) && supported(entry.path()) =>
            {
                if let Ok(relative) = entry.path().strip_prefix(folder) {
                    paths.push(relative.to_path_buf());
                }
            }
            Ok(entry) if entry.depth() == 16 && entry.file_type().is_some_and(|t| t.is_dir()) => {
                partial = true
            }
            Err(_) => partial = true,
            _ => {}
        }
    }
    Ok((paths, partial))
}

enum StartError {
    Track(String),
    Device(String),
}

trait Audio {
    fn start(
        &mut self,
        path: &Path,
        volume: u8,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(), StartError>;
    fn pause(&self);
    fn resume(&self);
    fn volume(&self, volume: u8);
    fn finished(&self) -> bool;
    fn failed(&self) -> bool;
    fn stop(&mut self);
}

#[derive(Default)]
struct NativeAudio {
    output: Option<MixerDeviceSink>,
    player: Option<Player>,
    failed: Arc<AtomicBool>,
}
impl Audio for NativeAudio {
    fn start(
        &mut self,
        path: &Path,
        volume: u8,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(), StartError> {
        self.player.take();
        let file = std::fs::File::open(path).map_err(|e| StartError::Track(e.to_string()))?;
        if !file
            .metadata()
            .map_err(|e| StartError::Track(e.to_string()))?
            .is_file()
        {
            return Err(StartError::Track(t("Not an audio file").to_string()));
        }
        let mut decoder = Decoder::try_from(file).map_err(|e| StartError::Track(e.to_string()))?;
        let first = decoder
            .next()
            .ok_or_else(|| StartError::Track(t("Audio file has no samples").to_string()))?;
        if cancel.load(Ordering::Acquire) {
            return Err(StartError::Track(t("Playback cancelled").to_string()));
        }
        if self.output.is_none() {
            self.failed.store(false, Ordering::Release);
            let failed = self.failed.clone();
            let mut output = DeviceSinkBuilder::from_default_device()
                .map_err(|e| StartError::Device(e.to_string()))?
                .with_error_callback(move |_| {
                    failed.store(true, Ordering::Release);
                })
                .open_stream()
                .map_err(|e| StartError::Device(e.to_string()))?;
            output.log_on_drop(false);
            self.output = Some(output);
        }
        if cancel.load(Ordering::Acquire) {
            self.stop();
            return Err(StartError::Track(t("Playback cancelled").to_string()));
        }
        let player = Player::connect_new(self.output.as_ref().unwrap().mixer());
        player.set_volume(f32::from(volume.min(100)) / 100.0);
        player.append(Cancellable {
            source: decoder,
            first: Some(first),
            cancelled: cancel.clone(),
        });
        self.player = Some(player);
        Ok(())
    }
    fn pause(&self) {
        if let Some(player) = &self.player {
            player.pause();
        }
    }
    fn resume(&self) {
        if let Some(player) = &self.player {
            player.play();
        }
    }
    fn volume(&self, volume: u8) {
        if let Some(player) = &self.player {
            player.set_volume(f32::from(volume.min(100)) / 100.0);
        }
    }
    fn finished(&self) -> bool {
        self.player.as_ref().is_none_or(Player::empty)
    }
    fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    fn stop(&mut self) {
        self.player.take();
        self.output.take();
    }
}

struct Cancellable<S> {
    source: S,
    first: Option<f32>,
    cancelled: Arc<AtomicBool>,
}
impl<S: Source> Iterator for Cancellable<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.cancelled.load(Ordering::Acquire) {
            None
        } else {
            self.first.take().or_else(|| self.source.next())
        }
    }
}
impl<S: Source> Source for Cancellable<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source
            .current_span_len()
            .map(|n| n + usize::from(self.first.is_some()))
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.source.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

fn playable_path(folder: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut path = folder.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(t("Invalid music path").to_string());
        };
        path.push(component);
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Err(t("Music path is a symlink").to_string());
        }
    }
    Ok(path)
}

#[allow(clippy::too_many_arguments)]
fn run<B: Audio>(
    mut queue: Queue,
    partial: bool,
    mut volume: u8,
    folder: &Path,
    receiver: mpsc::Receiver<Command>,
    cancel: &Arc<AtomicBool>,
    repeat_track: &AtomicBool,
    updates: &Updates,
    mut audio: B,
) {
    let mut state = if queue.current().is_some() {
        State::Paused
    } else {
        State::Empty
    };
    let mut loaded = false;
    let mut error = None;
    let mut published = None;
    let mut failed_tracks = std::collections::HashSet::new();
    loop {
        if cancel.load(Ordering::Acquire) {
            break;
        }
        let status = Status {
            state,
            track: queue.current().map(Path::to_path_buf),
            count: queue.tracks.len(),
            partial,
            error: error.clone(),
        };
        if published.as_ref() != Some(&status) {
            updates.send(status.clone());
            published = Some(status);
        }
        // Only playing needs an end-of-track check. Paused/idle waits indefinitely
        // for a command, with no timers or GPUI redraws.
        let command = if state == State::Playing {
            match receiver.recv_timeout(Duration::from_millis(200)) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if audio.failed() {
                        audio.stop();
                        loaded = false;
                        state = State::Error;
                        error = Some(t("Audio output became unavailable").to_string());
                        continue;
                    }
                    if !audio.finished() {
                        continue;
                    }
                    loaded = false;
                    queue.track_ended(repeat_track.load(Ordering::Acquire));
                    Command::Play
                }
            }
        } else {
            match receiver.recv() {
                Ok(command) => command,
                Err(_) => break,
            }
        };
        if cancel.load(Ordering::Acquire) {
            break;
        }
        match command {
            Command::Stop => break,
            Command::Volume(value) => {
                volume = value;
                audio.volume(volume);
            }
            Command::Pause => {
                audio.pause();
                if state == State::Playing {
                    state = State::Paused;
                }
            }
            Command::Skip | Command::Previous => {
                let playing = state == State::Playing;
                audio.stop();
                loaded = false;
                if matches!(command, Command::Previous) {
                    queue.previous();
                } else {
                    queue.advance();
                }
                if !playing {
                    state = if queue.current().is_some() {
                        State::Paused
                    } else {
                        State::Empty
                    };
                    continue;
                }
                state = State::Playing;
            }
            Command::Play => {
                error = None;
                if loaded && !audio.failed() {
                    audio.resume();
                    state = State::Playing;
                    continue;
                }
                audio.stop();
                loaded = false;
                state = State::Playing;
            }
        }
        if state == State::Playing && !loaded {
            let mut failure = None;
            for _ in 0..queue.tracks.len() {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                let relative = queue.current().unwrap().to_path_buf();
                if failed_tracks.contains(&relative) {
                    queue.advance();
                    continue;
                }
                let result = playable_path(folder, &relative)
                    .map_err(StartError::Track)
                    .and_then(|path| audio.start(&path, volume, cancel));
                match result {
                    Ok(()) => {
                        loaded = true;
                        break;
                    }
                    Err(StartError::Track(e)) => {
                        failure = Some(e);
                        failed_tracks.insert(relative);
                        queue.advance();
                    }
                    Err(StartError::Device(e)) => {
                        failure = Some(e);
                        break;
                    }
                }
            }
            if !loaded {
                state = if queue.tracks.is_empty() {
                    State::Empty
                } else {
                    State::Error
                };
                error = failure;
                audio.stop();
            }
        }
    }
    audio.stop();
}

#[cfg(test)]
mod tests {
    use super::{Audio, Command, State, Updates, discover, playable_path, run};
    use muxal_core::music::Queue;
    use std::{
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };
    #[derive(Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<&'static str>>>,
        paths: Arc<Mutex<Vec<PathBuf>>>,
        ended: Arc<AtomicBool>,
        failure: Option<bool>,
    }
    impl Audio for Fake {
        fn start(
            &mut self,
            path: &Path,
            _: u8,
            _: &Arc<AtomicBool>,
        ) -> Result<(), super::StartError> {
            self.calls.lock().unwrap().push("start");
            self.paths.lock().unwrap().push(path.to_path_buf());
            match self.failure {
                Some(true) => Err(super::StartError::Device("No output".into())),
                Some(false) => Err(super::StartError::Track("Invalid track".into())),
                None => Ok(()),
            }
        }
        fn pause(&self) {
            self.calls.lock().unwrap().push("pause");
        }
        fn resume(&self) {
            self.calls.lock().unwrap().push("resume");
        }
        fn volume(&self, _: u8) {}
        fn finished(&self) -> bool {
            self.ended.swap(false, Ordering::AcqRel)
        }
        fn failed(&self) -> bool {
            false
        }
        fn stop(&mut self) {
            self.calls.lock().unwrap().push("stop");
        }
    }
    #[test]
    fn paused_worker_is_lazy_and_skip_keeps_pause() {
        let dir = std::env::temp_dir().join(format!("muxal-music-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.wav"), b"fixture").unwrap();
        std::fs::write(dir.join("b.wav"), b"fixture").unwrap();
        let (tx, rx) = mpsc::sync_channel(8);
        let (events, latest) = async_channel::bounded(1);
        let fake = Fake::default();
        let calls = fake.calls.clone();
        let root = dir.clone();
        let read = latest.clone();
        let worker = std::thread::spawn(move || {
            run(
                Queue::new(vec!["a.wav".into(), "b.wav".into()], None),
                false,
                35,
                &root,
                rx,
                &Arc::new(AtomicBool::new(false)),
                &AtomicBool::new(false),
                &Updates { events, latest },
                fake,
            )
        });
        let initial = read.recv_blocking().unwrap();
        assert_eq!(initial.state, State::Paused);
        assert!(calls.lock().unwrap().is_empty());
        tx.send(Command::Skip).unwrap();
        let skipped = read.recv_blocking().unwrap();
        assert_eq!(skipped.state, State::Paused);
        assert_eq!(skipped.track.as_deref(), Some(Path::new("b.wav")));
        assert!(!calls.lock().unwrap().contains(&"start"));
        tx.send(Command::Previous).unwrap();
        let previous = read.recv_blocking().unwrap();
        assert_eq!(previous.state, State::Paused);
        assert_eq!(previous.track.as_deref(), Some(Path::new("a.wav")));
        tx.send(Command::Previous).unwrap();
        assert_eq!(
            read.recv_blocking().unwrap().track.as_deref(),
            Some(Path::new("b.wav"))
        );
        assert!(!calls.lock().unwrap().contains(&"start"));
        tx.send(Command::Play).unwrap();
        assert_eq!(read.recv_blocking().unwrap().state, State::Playing);
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(
            read.try_recv().is_err(),
            "Playing must not publish a progress clock"
        );
        tx.send(Command::Pause).unwrap();
        assert_eq!(read.recv_blocking().unwrap().state, State::Paused);
        tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
        assert!(calls.lock().unwrap().contains(&"pause"));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn repeat_reloads_current_track_but_manual_navigation_and_toggle_take_effect() {
        let dir = std::env::temp_dir().join(format!("muxal-music-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.wav", "b.wav"] {
            std::fs::write(dir.join(name), b"fixture").unwrap();
        }
        let (tx, rx) = mpsc::sync_channel(8);
        let (events, latest) = async_channel::bounded(1);
        let fake = Fake::default();
        let paths = fake.paths.clone();
        let ended = fake.ended.clone();
        let repeat = Arc::new(AtomicBool::new(true));
        let worker_repeat = repeat.clone();
        let root = dir.clone();
        let read = latest.clone();
        let worker = std::thread::spawn(move || {
            run(
                Queue::new(vec!["a.wav".into(), "b.wav".into()], None),
                false,
                35,
                &root,
                rx,
                &Arc::new(AtomicBool::new(false)),
                &worker_repeat,
                &Updates { events, latest },
                fake,
            );
        });
        assert_eq!(read.recv_blocking().unwrap().state, State::Paused);
        tx.send(Command::Play).unwrap();
        assert_eq!(read.recv_blocking().unwrap().state, State::Playing);
        ended.store(true, Ordering::Release);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while paths.lock().unwrap().len() < 2 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            *paths.lock().unwrap(),
            vec![dir.join("a.wav"), dir.join("a.wav")]
        );
        assert!(
            read.try_recv().is_err(),
            "Repeating needs no redundant UI update"
        );
        tx.send(Command::Skip).unwrap();
        let next = read.recv_blocking().unwrap();
        assert_eq!(next.state, State::Playing);
        assert_eq!(next.track.as_deref(), Some(Path::new("b.wav")));
        tx.send(Command::Previous).unwrap();
        assert_eq!(
            read.recv_blocking().unwrap().track.as_deref(),
            Some(Path::new("a.wav"))
        );
        repeat.store(false, Ordering::Release);
        ended.store(true, Ordering::Release);
        let next = read.recv_blocking().unwrap();
        assert_eq!(next.state, State::Playing);
        assert_eq!(next.track.as_deref(), Some(Path::new("b.wav")));
        tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn discovery_is_read_only_bounded_to_music_and_cancellable() {
        let dir = std::env::temp_dir().join(format!("muxal-music-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/track.MP3"), b"fixture").unwrap();
        std::fs::write(dir.join(".hidden.wav"), b"fixture").unwrap();
        std::fs::write(dir.join("note.txt"), b"fixture").unwrap();
        let (paths, partial) = discover(&dir, &AtomicBool::new(false)).unwrap();
        assert_eq!(paths, vec![std::path::PathBuf::from("sub/track.MP3")]);
        assert!(!partial);
        assert!(discover(&dir, &AtomicBool::new(true)).unwrap().0.is_empty());
        assert!(playable_path(&dir, Path::new("../other.wav")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("sub", dir.join("link")).unwrap();
            assert!(playable_path(&dir, Path::new("link/track.MP3")).is_err());
            assert_eq!(discover(&dir, &AtomicBool::new(false)).unwrap().0.len(), 1);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn disabled_service_rejects_start_without_creating_a_worker() {
        assert!(super::Service::start(muxal_core::music::MusicSettings::default()).is_err());
    }
    #[test]
    fn all_promised_formats_decode_without_an_audio_device() {
        let fixtures: &[&[u8]] = &[
            include_bytes!("../tests/fixtures/music/silence.wav"),
            include_bytes!("../tests/fixtures/music/silence.mp3"),
            include_bytes!("../tests/fixtures/music/silence.flac"),
            include_bytes!("../tests/fixtures/music/silence.ogg"),
        ];
        for bytes in fixtures {
            let decoder = rodio::Decoder::try_from(std::io::Cursor::new(*bytes)).unwrap();
            let samples: Vec<_> = decoder.take(100_000).collect();
            assert!(!samples.is_empty());
            assert!(samples.len() < 100_000);
            assert!(samples.iter().all(|x| x.is_finite()));
        }
        assert!(rodio::Decoder::try_from(std::io::Cursor::new(vec![0u8; 32])).is_err());
    }
    #[test]
    fn invalid_tracks_are_bounded_and_missing_device_is_not_retried_per_track() {
        for device_failure in [false, true] {
            let dir = std::env::temp_dir().join(format!("muxal-music-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            for name in ["a.wav", "b.wav"] {
                std::fs::write(dir.join(name), b"fixture").unwrap();
            }
            let (tx, rx) = mpsc::sync_channel(8);
            let (events, latest) = async_channel::bounded(1);
            let fake = Fake {
                failure: Some(device_failure),
                ..Fake::default()
            };
            let calls = fake.calls.clone();
            let root = dir.clone();
            let read = latest.clone();
            let worker = std::thread::spawn(move || {
                run(
                    Queue::new(vec!["a.wav".into(), "b.wav".into()], None),
                    false,
                    35,
                    &root,
                    rx,
                    &Arc::new(AtomicBool::new(false)),
                    &AtomicBool::new(false),
                    &Updates { events, latest },
                    fake,
                )
            });
            assert_eq!(read.recv_blocking().unwrap().state, State::Paused);
            tx.send(Command::Play).unwrap();
            let failure = read.recv_blocking().unwrap();
            assert_eq!(failure.state, State::Error);
            assert!(failure.error.is_some());
            tx.send(Command::Stop).unwrap();
            worker.join().unwrap();
            assert_eq!(
                calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|c| **c == "start")
                    .count(),
                if device_failure { 1 } else { 2 }
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn native_audio_lifecycle_uses_only_an_isolated_silent_device() {
        let dir = std::env::temp_dir().join(format!("muxal-audio-null-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("asound.conf");
        std::fs::write(&config, "pcm.!default { type null }\n").unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "music::tests::native_audio_child", "--nocapture"])
            .env("ALSA_CONFIG_PATH", &config)
            .env("MUXAL_MUSIC_NULL_TEST", "1")
            .output()
            .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn native_audio_child() {
        if std::env::var("MUXAL_MUSIC_NULL_TEST").as_deref() != Ok("1") {
            return;
        }
        let config = std::env::var("ALSA_CONFIG_PATH").unwrap();
        assert_eq!(
            std::fs::read_to_string(config).unwrap(),
            "pcm.!default { type null }\n"
        );
        let mut audio = super::NativeAudio::default();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/music/silence.wav");
        let cancelled = Arc::new(AtomicBool::new(false));
        assert!(audio.start(&path, 35, &cancelled).is_ok());
        assert!(audio.output.is_some() && audio.player.is_some());
        audio.pause();
        audio.volume(15);
        audio.resume();
        audio.stop();
        assert!(audio.output.is_none() && audio.player.is_none());
    }
    #[test]
    fn generated_wav_decodes_and_disabled_source_stops() {
        use rodio::Source;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&40u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&[0; 4]);
        let decoder = rodio::Decoder::try_from(std::io::Cursor::new(wav)).unwrap();
        assert_eq!(decoder.sample_rate().get(), 8000);
        let cancel = Arc::new(AtomicBool::new(false));
        let mut source = super::Cancellable {
            source: decoder,
            first: None,
            cancelled: cancel.clone(),
        };
        assert!(source.next().is_some());
        cancel.store(true, std::sync::atomic::Ordering::Release);
        assert!(source.next().is_none());
    }
}
