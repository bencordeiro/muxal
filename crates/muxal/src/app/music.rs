use super::*;
use crate::music::{Command, Service, State, Status};

#[derive(Default)]
pub(super) struct MusicUi {
    service: Option<Service>,
    folder: Option<PathBuf>,
    generation: Uuid,
    task: Option<Task<()>>,
    status: Status,
    autoplay: muxal_core::music::Autoplay,
}

impl MusicUi {
    pub(super) fn service_stop(&mut self) {
        self.service.take();
        self.task.take();
    }
}

impl MuxalApp {
    pub(super) fn sync_music(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings.music.clone();
        if !settings.enabled && self.music.service.is_some() {
            self.music.autoplay.hold();
        }
        if !settings.enabled || settings.folder != self.music.folder {
            self.music.service.take();
            self.music.task.take();
            self.music.generation = Uuid::new_v4();
            self.music.folder = None;
            self.music.status = Status::default();
        }
        if !settings.enabled || settings.folder.is_none() {
            return;
        }
        if let Some(service) = &self.music.service {
            service.send(Command::Volume(settings.volume));
            service.set_repeat_track(settings.repeat_track);
            return;
        }
        let generation = Uuid::new_v4();
        self.music.generation = generation;
        self.music.folder = settings.folder.clone();
        match Service::start(settings) {
            Ok((service, events)) => {
                self.music.service = Some(service);
                self.music.task = Some(cx.spawn(async move |this, cx| {
                    while let Ok(status) = events.recv().await {
                        if this
                            .update(cx, |this, cx| {
                                if this.music.generation != generation
                                    || !this.settings.music.enabled
                                {
                                    return false;
                                }
                                if let Some(path) = &status.track
                                    && this.settings.music.last_track.as_ref() != Some(path)
                                {
                                    this.settings.music.last_track = Some(path.clone());
                                    this.persist_settings();
                                }
                                this.music.status = status;
                                cx.notify();
                                true
                            })
                            .ok()
                            != Some(true)
                        {
                            break;
                        }
                    }
                }));
            }
            Err(error) => {
                self.music.status = Status {
                    state: State::Error,
                    error: Some(error.to_string()),
                    ..Status::default()
                }
            }
        }
    }

    pub(super) fn music_project_opened(&mut self, cx: &mut Context<Self>) {
        if self.show_terms || self.current_workspace.is_none() || self.workspace.projects.is_empty()
        {
            return;
        }
        self.sync_music(cx);
        if self.music.autoplay.project_opened(&self.settings.music)
            && let Some(service) = &self.music.service
        {
            service.send(Command::Play);
        }
    }

    fn toggle_music(&mut self, cx: &mut Context<Self>) {
        if self.music.status.state == State::Playing {
            self.music.autoplay.hold();
            if let Some(service) = &self.music.service {
                service.send(Command::Pause);
            }
        } else {
            if self.music.status.state == State::Error {
                self.refresh_music(cx);
            }
            self.sync_music(cx);
            if let Some(service) = &self.music.service {
                service.send(Command::Play);
            }
        }
    }

    fn choose_music_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t("Choose music folder")),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut folders))) = chosen.await
                && let Some(folder) = folders.pop()
            {
                let _ = this.update(cx, |this, cx| {
                    if this.settings.music.folder.as_ref() == Some(&folder) {
                        this.refresh_music(cx);
                    } else {
                        this.settings.music.folder = Some(folder);
                        this.settings.music.last_track = None;
                        this.sync_music(cx);
                    }
                    this.persist_settings();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn refresh_music(&mut self, cx: &mut Context<Self>) {
        let playing = self.music.status.state == State::Playing;
        self.music.service.take();
        self.music.task.take();
        self.music.status = Status::default();
        self.sync_music(cx);
        if playing && let Some(service) = &self.music.service {
            service.send(Command::Play);
        }
        cx.notify();
    }

    pub(super) fn render_music_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.settings.music.enabled {
            return div().into_any_element();
        }
        let status = &self.music.status;
        let title = match status.state {
            State::Loading => t("Loading music…").to_string(),
            State::Empty => t("No supported music files").to_string(),
            State::Error => t("Music unavailable").to_string(),
            _ => status
                .track
                .as_deref()
                .and_then(Path::file_stem)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        let tooltip = status
            .error
            .clone()
            .or_else(|| status.track.as_ref().map(|p| p.display().to_string()))
            .unwrap_or_else(|| title.clone());
        let unavailable = matches!(status.state, State::Loading | State::Empty);
        let playing = status.state == State::Playing;
        let mut row = div()
            .px_2()
            .py_1()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .min_w_0()
            .w_full()
            .border_t_1()
            .border_color(cx.theme().border);
        if self.settings.music.folder.is_none() {
            row = row.child(
                Button::new("music-choose-footer")
                    .ghost()
                    .small()
                    .label(t("Choose music folder"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.choose_music_folder(window, cx)),
                    ),
            );
        } else {
            row = row
                .child(
                    div()
                        .id("music-track-label")
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                        })
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_xs()
                        .child(title),
                )
                .child(
                    Button::new("music-previous")
                        .ghost()
                        .small()
                        .icon(Icon::empty().path("icons/music-previous.svg"))
                        .tooltip(t("Previous track"))
                        .disabled(unavailable || status.count == 0)
                        .on_click(cx.listener(|this, _, _, _| {
                            if let Some(service) = &this.music.service {
                                service.send(Command::Previous);
                            }
                        })),
                )
                .child(
                    Button::new("music-play")
                        .ghost()
                        .small()
                        .icon(Icon::empty().path(if playing {
                            "icons/pause.svg"
                        } else {
                            "icons/music-play.svg"
                        }))
                        .tooltip(if playing {
                            t("Pause music")
                        } else {
                            t("Play music")
                        })
                        .disabled(unavailable)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_music(cx))),
                )
                .child(
                    Button::new("music-skip")
                        .ghost()
                        .small()
                        .icon(Icon::empty().path("icons/music-skip.svg"))
                        .tooltip(t("Next track"))
                        .disabled(unavailable || status.count == 0)
                        .on_click(cx.listener(|this, _, _, _| {
                            if let Some(service) = &this.music.service {
                                service.send(Command::Skip);
                            }
                        })),
                )
                .child(
                    Button::new("music-repeat")
                        .ghost()
                        .small()
                        .icon(Icon::empty().path("icons/music-repeat.svg"))
                        .tooltip(t("Repeat current track"))
                        .selected(self.settings.music.repeat_track)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.music.repeat_track = !this.settings.music.repeat_track;
                            if let Some(service) = &this.music.service {
                                service.set_repeat_track(this.settings.music.repeat_track);
                            }
                            this.persist_settings();
                            cx.notify();
                        })),
                );
        }
        row.into_any_element()
    }

    pub(super) fn render_settings_music(&self, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.settings.music.enabled;
        let mut form = v_flex().gap_3().max_w(px(520.0))
            .child(self.check_row(Checkbox::new("music-enabled").checked(enabled)
                .on_click(cx.listener(|this, enabled: &bool, _, cx| {
                    this.settings.music.enabled = *enabled;
                    if !enabled { this.music.autoplay.hold(); }
                    this.sync_music(cx); this.persist_settings(); cx.notify();
                })), &t("Enable local music")))
            .child(div().text_sm().text_color(cx.theme().muted_foreground)
                .child(t("Read music from a folder. Disabled by default; no scanning or audio device while disabled.")));
        if !enabled {
            return form.into_any_element();
        }
        let folder = self
            .settings
            .music
            .folder
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| t("No music folder selected").to_string());
        form = form.child(div().text_sm().overflow_hidden().child(folder))
            .child(div().flex().flex_wrap().gap_2()
                .child(Button::new("music-folder").small().label(t("Choose music folder"))
                    .on_click(cx.listener(|this, _, window, cx| this.choose_music_folder(window, cx))))
                .child(Button::new("music-refresh").small().label(t("Refresh folder")).disabled(self.settings.music.folder.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_music(cx)))))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(t("MP3, FLAC, PCM WAV, Ogg Vorbis. Includes subfolders; plays in filename order and repeats the folder.")))
            .child(self.check_row(Checkbox::new("music-autoplay").checked(self.settings.music.autoplay)
                .on_click(cx.listener(|this, enabled: &bool, _, cx| {
                    this.settings.music.autoplay = *enabled;
                    this.persist_settings(); cx.notify();
                })), &t("Start music when opening a project")))
            .child(div().text_xs().text_color(cx.theme().muted_foreground)
                .child(t("Starts once when a project opens. Music continues across project switches and while Muxal is in the background. A manual pause prevents later automatic starts.")))
            .child(self.settings_label(&t("Music volume"), cx));
        let mut volumes = div().flex().flex_wrap().gap_1();
        for volume in [15u8, 25, 35, 50, 75, 100] {
            volumes = volumes.child(
                Button::new(SharedString::from(format!("music-volume-{volume}")))
                    .ghost()
                    .small()
                    .label(format!("{volume}%"))
                    .selected(self.settings.music.volume == volume)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings.music.volume = volume;
                        this.sync_music(cx);
                        this.persist_settings();
                        cx.notify();
                    })),
            );
        }
        form = form.child(volumes);
        if self.music.status.partial {
            form = form.child(div().text_sm().child(t(
                "Music list is partial: a folder limit or unreadable path was encountered.",
            )));
        }
        if let Some(error) = &self.music.status.error {
            form = form
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(t("Music unavailable")),
                )
                .child(div().text_xs().child(error.clone()));
        }
        form.into_any_element()
    }
}
