//! Preferences and queue/autoplay decisions for optional local playback.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MusicSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub autoplay: bool,
    #[serde(default)]
    pub repeat_track: bool,
    #[serde(default)]
    pub folder: Option<PathBuf>,
    #[serde(default)]
    pub last_track: Option<PathBuf>,
    #[serde(default = "default_volume")]
    pub volume: u8,
}
fn default_volume() -> u8 {
    35
}
impl Default for MusicSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            autoplay: false,
            repeat_track: false,
            folder: None,
            last_track: None,
            volume: default_volume(),
        }
    }
}

/// Deliberate pause/disable wins over subsequent project-open events.
#[derive(Default)]
pub struct Autoplay {
    attempted: bool,
    held: bool,
}
impl Autoplay {
    pub fn project_opened(&mut self, settings: &MusicSettings) -> bool {
        if !settings.enabled
            || !settings.autoplay
            || settings.folder.is_none()
            || self.attempted
            || self.held
        {
            return false;
        }
        self.attempted = true;
        true
    }
    pub fn hold(&mut self) {
        self.held = true;
    }
}

#[derive(Default)]
pub struct Queue {
    pub tracks: Vec<PathBuf>,
    pub index: usize,
}
impl Queue {
    pub fn new(mut tracks: Vec<PathBuf>, remembered: Option<&Path>) -> Self {
        tracks.sort();
        tracks.dedup();
        let index = remembered
            .and_then(|p| tracks.iter().position(|t| t == p))
            .unwrap_or(0);
        Self { tracks, index }
    }
    pub fn current(&self) -> Option<&Path> {
        self.tracks.get(self.index).map(PathBuf::as_path)
    }
    pub fn advance(&mut self) {
        if !self.tracks.is_empty() {
            self.index = (self.index + 1) % self.tracks.len();
        }
    }
    pub fn previous(&mut self) {
        if !self.tracks.is_empty() {
            self.index = (self.index + self.tracks.len() - 1) % self.tracks.len();
        }
    }
    pub fn track_ended(&mut self, repeat_track: bool) {
        if !repeat_track {
            self.advance();
        }
    }
}

pub fn supported(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["mp3", "flac", "wav", "ogg"]
            .iter()
            .any(|s| e.eq_ignore_ascii_case(s))
    })
}

#[cfg(test)]
mod tests {
    use super::{Autoplay, MusicSettings, Queue, supported};
    use std::path::Path;
    #[test]
    fn old_preferences_are_opted_out_and_round_trip() {
        let old: MusicSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, MusicSettings::default());
        assert!(!old.enabled && !old.autoplay && !old.repeat_track);
        let enabled_repeat = MusicSettings {
            repeat_track: true,
            ..old.clone()
        };
        assert_eq!(
            serde_json::from_str::<MusicSettings>(&serde_json::to_string(&enabled_repeat).unwrap())
                .unwrap(),
            enabled_repeat
        );
        assert_eq!(
            serde_json::from_str::<MusicSettings>(&serde_json::to_string(&old).unwrap()).unwrap(),
            old
        );
    }
    #[test]
    fn autoplay_requires_opt_ins_and_only_attempts_once() {
        let mut gate = Autoplay::default();
        let mut settings = MusicSettings::default();
        assert!(!gate.project_opened(&settings));
        settings.enabled = true;
        settings.autoplay = true;
        assert!(!gate.project_opened(&settings));
        settings.folder = Some("music".into());
        assert!(gate.project_opened(&settings));
        assert!(!gate.project_opened(&settings));
        let mut held = Autoplay::default();
        held.hold();
        assert!(!held.project_opened(&settings));
    }
    #[test]
    fn queue_keeps_exact_paths_remembers_and_wraps() {
        let mut q = Queue::new(
            vec!["b.mp3".into(), "a.mp3".into(), "b.mp3".into()],
            Some(Path::new("b.mp3")),
        );
        assert_eq!(q.tracks.len(), 2);
        q.advance();
        assert_eq!(q.current(), Some(Path::new("a.mp3")));
        q.advance();
        assert_eq!(q.current(), Some(Path::new("b.mp3")));
        let mut empty = Queue::default();
        empty.advance();
        assert!(empty.current().is_none());
        assert!(supported(Path::new("track.MP3")));
        assert!(!supported(Path::new("track.m4a")));
    }
    #[test]
    fn previous_wraps_and_repeat_only_changes_natural_track_end() {
        let mut queue = Queue::new(vec!["a.mp3".into(), "b.mp3".into()], None);
        queue.previous();
        assert_eq!(queue.current(), Some(Path::new("b.mp3")));
        queue.track_ended(true);
        assert_eq!(queue.current(), Some(Path::new("b.mp3")));
        queue.track_ended(false);
        assert_eq!(queue.current(), Some(Path::new("a.mp3")));
        queue.advance();
        assert_eq!(queue.current(), Some(Path::new("b.mp3")));
        queue.previous();
        assert_eq!(queue.current(), Some(Path::new("a.mp3")));
        let mut empty = Queue::default();
        empty.previous();
        empty.track_ended(true);
        empty.track_ended(false);
        assert!(empty.current().is_none());
        let mut single = Queue::new(vec!["only.mp3".into()], None);
        single.previous();
        single.track_ended(false);
        single.track_ended(true);
        assert_eq!(single.current(), Some(Path::new("only.mp3")));
    }
}
