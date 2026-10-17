use std::sync::Arc;

use engine::{
    Config, Event, Lens, Mode, Provider, Report, Request, ResearchPlan, Researcher, Stratum,
    store::{SessionRow, Store},
};
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    text::markdown,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder, *};
use gpui_kit::base::Disableable;

use crate::{
    settings::{self, FIELDS},
    theme::{self, c},
};

const FEED_MAX: usize = 400;

pub struct DeepHoney {
    cfg: Config,
    store: Option<Store>,
    sessions: Vec<SessionRow>,
    topic: Entity<InputState>,
    mode: Mode,
    depth: usize,
    breadth: usize,
    feed: Vec<Event>,
    stats: [usize; 4],
    running: bool,
    planning: bool,
    preview: Option<(String, Mode, usize, usize)>,
    plan_fields: Vec<[Entity<InputState>; 4]>,
    pages_input: Entity<InputState>,
    round: Option<(usize, usize, usize, usize)>,
    abort: Option<tokio::task::AbortHandle>,
    error: Option<String>,
    report: Option<Report>,
    tab: usize,
    settings_open: bool,
    fields: Vec<Entity<InputState>>,
    rt: Arc<tokio::runtime::Runtime>,
    researcher: Arc<dyn Researcher>,
    task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl DeepHoney {
    pub fn new(
        rt: Arc<tokio::runtime::Runtime>,
        researcher: Arc<dyn Researcher>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cfg = settings::load();
        let topic =
            cx.new(|cx| InputState::new(window, cx).placeholder("What do you want to dig into?"));
        let sub = cx.subscribe_in(
            &topic,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => this.plan(window, cx),
                InputEvent::Change => {
                    this.invalidate_plan();
                    cx.notify();
                }
                _ => {}
            },
        );
        let mut probe = cfg.clone();
        let fields = FIELDS
            .iter()
            .map(|f| {
                let v = settings::slot(&mut probe, f.key).clone();
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .masked(f.secret)
                        .default_value(v)
                })
            })
            .collect();
        let (store, error) = match Store::open(&settings::dir().join("sessions.db")) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(format!("Could not open session store: {e}"))),
        };
        let pages_input =
            cx.new(|cx| InputState::new(window, cx).default_value(cfg.max_pages.to_string()));
        let mut this = Self {
            cfg,
            store,
            sessions: vec![],
            topic,
            mode: Mode::Normal,
            depth: 2,
            breadth: 3,
            feed: vec![],
            stats: [0; 4],
            running: false,
            planning: false,
            preview: None,
            plan_fields: vec![],
            pages_input,
            round: None,
            abort: None,
            error,
            report: None,
            tab: 0,
            settings_open: false,
            fields,
            rt,
            researcher,
            task: None,
            _subs: vec![sub],
        };
        this.refresh_sessions();
        this
    }

    fn refresh_sessions(&mut self) {
        if let Some(s) = &self.store {
            self.sessions = s.list().unwrap_or_default();
        }
    }

    fn identity(&self, cx: &App) -> (String, Mode, usize, usize) {
        (
            self.topic.read(cx).value().trim().to_string(),
            self.mode,
            self.depth,
            self.breadth,
        )
    }

    fn cancel(&mut self) {
        if let Some(task) = self.abort.take() {
            task.abort();
        }
        self.task = None;
        self.running = false;
        self.planning = false;
    }

    fn invalidate_plan(&mut self) {
        if self.planning {
            self.cancel();
        }
        self.preview = None;
        self.plan_fields.clear();
    }

    fn plan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let identity = self.identity(cx);
        if identity.0.is_empty() || self.running || self.planning {
            return;
        }
        let req = Request {
            topic: identity.0.clone(),
            mode: self.mode,
            depth: self.depth,
            breadth: self.breadth,
            plan: None,
        };
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let fut = self.researcher.plan(self.cfg.clone(), req);
        self.abort = Some(
            self.rt
                .spawn(async move {
                    let _ = done_tx.send(fut.await);
                })
                .abort_handle(),
        );
        self.planning = true;
        self.preview = None;
        self.plan_fields.clear();
        self.error = None;
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let res = done_rx
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("planning task stopped unexpectedly")));
            let _ = this.update_in(cx, |v, window, cx| {
                v.planning = false;
                v.abort = None;
                if v.identity(cx) != identity {
                    return;
                }
                match res {
                    Ok(plan) if !plan.lenses.is_empty() => {
                        v.plan_fields = plan
                            .lenses
                            .into_iter()
                            .map(|lens| {
                                [
                                    lens.name,
                                    lens.question,
                                    lens.sources.join(", "),
                                    lens.queries.join("; "),
                                ]
                                .map(|value| {
                                    cx.new(|cx| InputState::new(window, cx).default_value(value))
                                })
                            })
                            .collect();
                        v.preview = Some(identity);
                    }
                    Ok(_) => {
                        v.error = Some(
                            "The planner returned no lenses. Try a more specific topic.".into(),
                        )
                    }
                    Err(e) => v.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn run(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.running || self.planning || self.preview.as_ref() != Some(&self.identity(cx)) {
            return;
        }
        let lenses: Vec<Lens> = self
            .plan_fields
            .iter()
            .map(|fields| Lens {
                name: fields[0].read(cx).value().trim().to_string(),
                question: fields[1].read(cx).value().trim().to_string(),
                sources: fields[2]
                    .read(cx)
                    .value()
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
                    .collect(),
                queries: fields[3]
                    .read(cx)
                    .value()
                    .split(';')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
            .collect();
        if lenses.is_empty()
            || lenses.len() > 8
            || lenses.iter().any(|lens| {
                lens.name.is_empty()
                    || lens.question.is_empty()
                    || lens.queries.is_empty()
                    || lens.sources.is_empty()
            })
        {
            self.error = Some("Every lens needs a name, question, source and query.".into());
            cx.notify();
            return;
        }
        let available = engine::sources::Source::for_mode(self.mode, &self.cfg);
        if lenses.iter().any(|lens| {
            lens.sources
                .iter()
                .any(|name| !available.iter().any(|source| source.name() == name))
        }) {
            self.error = Some("Use enabled source IDs from the list below the plan.".into());
            cx.notify();
            return;
        }
        let req = Request {
            topic: self.identity(cx).0,
            mode: self.mode,
            depth: self.depth,
            breadth: self.breadth,
            plan: Some(ResearchPlan {
                done: false,
                lenses,
            }),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let fut = self.researcher.run(self.cfg.clone(), req, tx);
        self.abort = Some(
            self.rt
                .spawn(async move {
                    let _ = done_tx.send(fut.await);
                })
                .abort_handle(),
        );
        self.running = true;
        self.error = None;
        self.feed.clear();
        self.stats = [0; 4];
        self.round = None;
        self.settings_open = false;
        self.task = Some(cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.recv().await {
                if this.update(cx, |v, cx| v.on_event(ev, cx)).is_err() {
                    return;
                }
            }
            let res = done_rx
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("research task stopped unexpectedly")));
            let _ = this.update(cx, |v, cx| {
                v.running = false;
                v.abort = None;
                match res {
                    Ok(r) => {
                        if let Some(s) = &v.store
                            && let Err(e) = s.save(&r)
                        {
                            v.error = Some(format!("Could not save session: {e}"));
                        }
                        v.refresh_sessions();
                        v.report = Some(r);
                        v.preview = None;
                        v.plan_fields.clear();
                        v.tab = 0;
                    }
                    Err(e) => v.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        if let Event::Stats {
            pages,
            sources,
            voices,
            claims,
        } = ev
        {
            self.stats = [pages, sources, voices, claims];
        } else {
            if let Event::Round {
                round,
                new_claims,
                pages,
                remaining,
            } = &ev
            {
                self.round = Some((*round, *new_claims, *pages, *remaining));
            }
            self.feed.push(ev);
            if self.feed.len() > FEED_MAX {
                self.feed.remove(0);
            }
        }
        cx.notify();
    }

    fn open_session(&mut self, id: i64, cx: &mut Context<Self>) {
        self.cancel();
        self.invalidate_plan();
        self.stats = [0; 4];
        self.round = None;
        let Some(s) = &self.store else { return };
        match s.load(id) {
            Ok(r) => {
                self.report = Some(r);
                self.feed.clear();
                self.error = None;
                self.settings_open = false;
                self.tab = 0;
            }
            Err(e) => self.error = Some(format!("Could not load session: {e}")),
        }
        cx.notify();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let pages = match settings::page_budget(self.pages_input.read(cx).value().as_ref()) {
            Ok(pages) => pages,
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        self.cfg.max_pages = pages;
        self.invalidate_plan();
        for (f, input) in FIELDS.iter().zip(&self.fields) {
            *settings::slot(&mut self.cfg, f.key) = input.read(cx).value().trim().to_string();
        }
        match settings::save(&self.cfg) {
            Ok(()) => self.settings_open = false,
            Err(e) => self.error = Some(format!("Could not save settings: {e}")),
        }
        cx.notify();
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self.sessions.iter().map(|s| {
            let id = s.id;
            let active = self.report.as_ref().is_some_and(|r| r.topic == s.topic);
            div()
                .id(("session", id as usize))
                .px_3()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .when(active, |d| d.bg(c(theme::RAISED)))
                .hover(|d| d.bg(c(theme::RAISED)))
                .child(
                    div()
                        .text_sm()
                        .text_color(c(theme::TEXT))
                        .truncate()
                        .child(s.topic.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(c(theme::MUTED))
                        .child(format!("{} · {}", s.mode, s.created)),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_session(id, cx)))
        });
        v_flex()
            .w(px(260.))
            .h_full()
            .flex_none()
            .bg(c(theme::PANEL))
            .border_r_1()
            .border_color(c(theme::LINE))
            .pt(px(44.))
            .px_3()
            .pb_3()
            .gap_3()
            .child(
                h_flex()
                    .px_2()
                    .gap_2()
                    .child(div().text_xl().text_color(c(theme::HONEY)).child("⬢"))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("DeepHoney"),
                    ),
            )
            .child(
                h_flex().child(
                    Button::new("new")
                        .label("New research")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cancel();
                            this.invalidate_plan();
                            this.stats = [0; 4];
                            this.round = None;
                            this.report = None;
                            this.feed.clear();
                            this.error = None;
                            this.settings_open = false;
                            this.topic.update(cx, |t, cx| t.set_value("", window, cx));
                            cx.notify();
                        })),
                ),
            )
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(c(theme::MUTED))
                    .child("SESSIONS"),
            )
            .child(
                v_flex()
                    .id("sessions")
                    .flex_1()
                    .overflow_y_scroll()
                    .gap_1()
                    .children(list)
                    .when(self.sessions.is_empty(), |d| {
                        d.child(
                            div()
                                .px_2()
                                .text_sm()
                                .text_color(c(theme::MUTED))
                                .child("Finished research lands here."),
                        )
                    }),
            )
            .child(
                h_flex().child(
                    Button::new("settings")
                        .label("Settings")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings_open = !this.settings_open;
                            cx.notify();
                        })),
                ),
            )
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let modes = Mode::ALL.into_iter().enumerate().map(|(i, m)| {
            Button::new(("mode", i))
                .label(m.label())
                .small()
                .when(self.mode == m, |b| b.primary())
                .when(self.mode != m, |b| b.ghost())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.mode != m {
                        this.invalidate_plan();
                    }
                    this.mode = m;
                    cx.notify();
                }))
        });
        v_flex()
            .gap_3()
            .p_4()
            .bg(c(theme::PANEL))
            .border_1()
            .border_color(c(theme::LINE))
            .rounded_lg()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.topic).large()))
                    .child(
                        Button::new("plan")
                            .primary()
                            .large()
                            .label(if self.planning { "Planning…" } else { "Plan" })
                            .loading(self.planning)
                            .disabled(self.running || self.planning)
                            .on_click(cx.listener(|this, _, window, cx| this.plan(window, cx))),
                    )
                    .when(self.running || self.planning, |row| {
                        row.child(Button::new("stop").label("Cancel").ghost().on_click(
                            cx.listener(|this, _, _, cx| {
                                this.cancel();
                                cx.notify();
                            }),
                        ))
                    }),
            )
            .child(
                h_flex()
                    .gap_4()
                    .flex_wrap()
                    .child(h_flex().gap_1().children(modes))
                    .child(div().flex_1())
                    .child(self.stepper("Depth", self.depth, 1, 5, |t, v| t.depth = v, cx))
                    .child(self.stepper("Breadth", self.breadth, 1, 6, |t, v| t.breadth = v, cx)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(c(theme::MUTED))
                    .child(mode_hint(self.mode)),
            )
    }

    fn plan_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let fields = self.plan_fields.iter().enumerate().map(|(i, inputs)| {
            v_flex()
                .gap_2()
                .p_3()
                .border_b_1()
                .border_color(c(theme::LINE))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Lens {}", i + 1)),
                )
                .children(
                    [
                        "Name",
                        "Subquestion",
                        "Source IDs (comma separated)",
                        "Queries (semicolon separated)",
                    ]
                    .into_iter()
                    .zip(inputs)
                    .map(|(label, input)| {
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().text_color(c(theme::MUTED)).child(label))
                            .child(Input::new(input).disabled(self.running))
                    }),
                )
        });
        let sources = engine::sources::Source::for_mode(self.mode, &self.cfg)
            .iter()
            .map(|source| source.name())
            .collect::<Vec<_>>()
            .join(", ");
        v_flex().id("plan-preview").flex_1().min_h_0().overflow_y_scroll().gap_3().p_4().rounded_lg().bg(c(theme::PANEL))
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Review your research plan"))
            .child(div().text_sm().text_color(c(theme::MUTED)).child("Planning uses the configured model. Searches and page reading start only after Run."))
            .child(div().text_sm().child(format!("Budget: up to {} page reads and {} search calls across {} rounds. Model and search charges depend on your providers and usage.", self.cfg.max_pages, Request { topic: String::new(), mode: self.mode, depth: self.depth, breadth: self.breadth, plan: None }.search_budget(), self.depth)))
            .children(fields)
            .child(div().text_xs().text_color(c(theme::MUTED)).child(format!("Enabled source IDs: {sources}")))
            .child(Button::new("run-edited-plan").label("Run this plan").primary().disabled(self.running).on_click(cx.listener(|this, _, window, cx| this.run(window, cx))))
    }

    fn stepper(
        &self,
        label: &'static str,
        value: usize,
        min: usize,
        max: usize,
        set: fn(&mut Self, usize),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(c(theme::MUTED))
                    .mr_1()
                    .child(label),
            )
            .child(
                Button::new((label, 0usize))
                    .label("−")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.invalidate_plan();
                        set(this, value.saturating_sub(1).max(min));
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .w(px(18.))
                    .text_center()
                    .text_sm()
                    .child(value.to_string()),
            )
            .child(
                Button::new((label, 1usize))
                    .label("+")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.invalidate_plan();
                        set(this, (value + 1).min(max));
                        cx.notify();
                    })),
            )
    }

    fn feed_panel(&self) -> impl IntoElement {
        let [pages, sources, voices, claims] = self.stats;
        let stat = |n: usize, l: &'static str| {
            v_flex()
                .flex_1()
                .p_2()
                .rounded_md()
                .bg(c(theme::RAISED))
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(theme::HONEY))
                        .child(n.to_string()),
                )
                .child(div().text_xs().text_color(c(theme::MUTED)).child(l))
        };
        let rows = self
            .feed
            .iter()
            .rev()
            .enumerate()
            .map(|(i, e)| feed_row(i, e));
        v_flex()
            .w(px(360.))
            .flex_none()
            .h_full()
            .gap_3()
            .p_4()
            .bg(c(theme::PANEL))
            .border_1()
            .border_color(c(theme::LINE))
            .rounded_lg()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().size(px(8.)).rounded_full().bg(c(if self.running { theme::HONEY } else { theme::LINE })))
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Live hive")),
            )
            .child(h_flex().gap_2().child(stat(pages, "pages")).child(stat(sources, "sources")).child(stat(voices, "voices")).child(stat(claims, "claims")))
            .when_some(self.round, |panel, (round, new_claims, pages, remaining)| panel.child(div().text_xs().text_color(c(theme::MUTED)).child(format!("Round {round} · {new_claims} new claims / {pages} pages · {:.2} new claims/page · {remaining} reads left", new_claims as f64 / pages.max(1) as f64))))
            .child(
                v_flex()
                    .id("feed")
                    .flex_1()
                    .overflow_y_scroll()
                    .gap_1()
                    .children(rows)
                    .when(self.feed.is_empty(), |d| {
                        d.child(div().text_sm().text_color(c(theme::MUTED)).child("Queries, pages read and claims stream here while a run is active."))
                    }),
            )
    }

    fn results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = h_flex().gap_1().p_2().border_b_1().border_color(c(theme::LINE)).children(
            ["Report", "Claims", "Sources", "Deleted passages"].into_iter().enumerate().map(|(i, label)| {
                Button::new(("result-tab", i)).label(label).small()
                    .when(self.tab == i, |b| b.primary()).when(self.tab != i, |b| b.ghost())
                    .on_click(cx.listener(move |this, _, _, cx| { this.tab = i; cx.notify(); }))
            })
        );
        let body: AnyElement = match &self.report {
            None => empty_state(self.error.is_some()).into_any_element(),
            Some(r) => match self.tab {
                0 => div()
                    .p_4()
                    .child(markdown(r.markdown.clone()).selectable(true))
                    .into_any_element(),
                1 => claims_view(r).into_any_element(),
                2 => sources_view(r).into_any_element(),
                _ => ghosts_view(r).into_any_element(),
            },
        };
        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .bg(c(theme::PANEL))
            .border_1()
            .border_color(c(theme::LINE))
            .rounded_lg()
            .overflow_hidden()
            .child(tabs)
            .child(div().id("results").flex_1().overflow_y_scroll().child(body))
    }

    fn settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let anthropic = self.cfg.provider == Provider::Anthropic;
        let rows = FIELDS.iter().zip(&self.fields).map(|(f, input)| {
            v_flex()
                .gap_1()
                .child(div().text_xs().text_color(c(theme::MUTED)).child(f.label))
                .child(Input::new(input).when(f.secret, |i| i.mask_toggle()))
        });
        v_flex()
            .id("settings")
            .flex_1()
            .overflow_y_scroll()
            .gap_4()
            .p_5()
            .bg(c(theme::PANEL))
            .border_1()
            .border_color(c(theme::LINE))
            .rounded_lg()
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Settings"))
            .child(div().text_sm().text_color(c(theme::MUTED)).child("Keys are stored in the macOS Keychain. Everything else is saved to ~/Library/Application Support/DeepHoney."))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child("LLM provider"))
                    .child(Button::new("p-a").label("Anthropic").small().when(anthropic, |b| b.primary()).when(!anthropic, |b| b.ghost()).on_click(cx.listener(|this, _, _, cx| {
                        this.cfg.provider = Provider::Anthropic;
                        cx.notify();
                    })))
                    .child(Button::new("p-o").label("OpenAI-compatible").small().when(!anthropic, |b| b.primary()).when(anthropic, |b| b.ghost()).on_click(cx.listener(|this, _, _, cx| {
                        this.cfg.provider = Provider::OpenAi;
                        cx.notify();
                    }))),
            )
            .children(rows)
            .child(v_flex().gap_1().child(div().text_xs().text_color(c(theme::MUTED)).child("Maximum page reads (1–1000)")).child(Input::new(&self.pages_input)))
            .child(Button::new("tor-opt-in").label(if self.cfg.tor_enabled { "Tor enabled — click to disable" } else { "Enable embedded Tor for .onion text" }).small().when(self.cfg.tor_enabled, |b| b.primary()).when(!self.cfg.tor_enabled, |b| b.ghost()).on_click(cx.listener(|this, _, _, cx| {
                this.cfg.tor_enabled = !this.cfg.tor_enabled;
                this.invalidate_plan();
                cx.notify();
            })))
            .child(div().text_xs().text_color(c(theme::MUTED)).child("Explicit opt-in: .onion text uses built-in Tor. LLM and public search APIs connect directly. Local history stores fetched text."))
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("save").primary().label("Save").on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))))
                    .child(Button::new("cancel").ghost().label("Close").on_click(cx.listener(|this, _, _, cx| {
                        this.settings_open = false;
                        cx.notify();
                    }))),
            )
    }
}

impl Render for DeepHoney {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = if self.settings_open {
            self.settings_panel(cx).into_any_element()
        } else if self.preview.is_some() && !self.running {
            self.plan_panel(cx).into_any_element()
        } else {
            h_flex()
                .flex_1()
                .min_h_0()
                .gap_3()
                .items_start()
                .child(self.feed_panel())
                .child(self.results(cx))
                .into_any_element()
        };
        h_flex()
            .size_full()
            .bg(c(theme::BG))
            .text_color(cx.theme().foreground)
            .child(self.sidebar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pt(px(36.))
                    .px_4()
                    .pb_4()
                    .gap_3()
                    .child(self.composer(cx))
                    .when_some(self.error.clone(), |d, e| {
                        d.child(
                            div()
                                .px_4()
                                .py_2()
                                .rounded_md()
                                .border_1()
                                .border_color(c(theme::DANGER))
                                .text_sm()
                                .text_color(c(theme::DANGER))
                                .child(e),
                        )
                    })
                    .child(main),
            )
    }
}

fn mode_hint(m: Mode) -> &'static str {
    match m {
        Mode::Normal => "Web search plus scholarly, filings, court, code and community sources.",
        Mode::Hidden => {
            "Adds archives: dead-link recovery, deleted-passage diffs, and document-type operators."
        }
        Mode::Dark => {
            "Adds Ahmia discovery and text-only .onion reads through embedded Tor. Enable Tor in Settings first."
        }
        Mode::Adversarial => "Adds a skeptic pass that tries to falsify the strongest claims.",
    }
}

fn dot(color: Hsla) -> Div {
    div()
        .mt(px(6.))
        .size(px(6.))
        .flex_none()
        .rounded_full()
        .bg(color)
}

fn feed_row(i: usize, e: &Event) -> impl IntoElement {
    let (color, head, body): (Hsla, String, String) = match e {
        Event::Plan(plan) => (
            c(theme::HONEY),
            "plan".into(),
            format!("{} lenses", plan.lenses.len()),
        ),
        Event::Round {
            round,
            new_claims,
            pages,
            remaining,
        } => (
            c(theme::HONEY),
            format!("round {round}"),
            format!("{new_claims} new claims from {pages} pages; {remaining} reads remaining"),
        ),
        Event::Status(s) => (c(theme::MUTED), "status".into(), s.clone()),
        Event::Query {
            lens,
            source,
            stratum,
            query,
        } => (
            theme::stratum(*stratum),
            format!("{lens} · {source}"),
            query.clone(),
        ),
        Event::Page {
            lens,
            stratum,
            title,
            url,
        } => (
            theme::stratum(*stratum),
            format!("read · {lens}"),
            if title.is_empty() {
                url.clone()
            } else {
                title.clone()
            },
        ),
        Event::Claim { lens, text } => (c(theme::HONEY), format!("claim · {lens}"), text.clone()),
        Event::Pivot { kind, value } => (c(0x34d399), format!("pivot · {kind}"), value.clone()),
        Event::Ghost { url, removed } => (
            theme::stratum(Stratum::Buried),
            format!("deleted · {removed} passages"),
            url.clone(),
        ),
        Event::Warn(s) => (c(theme::DANGER), "warning".into(), s.clone()),
        Event::Stats { .. } => (c(theme::MUTED), String::new(), String::new()),
    };
    h_flex()
        .id(("feed-row", i))
        .items_start()
        .gap_2()
        .child(dot(color))
        .child(
            v_flex()
                .min_w_0()
                .child(div().text_xs().text_color(color).child(head))
                .child(
                    div()
                        .text_sm()
                        .text_color(c(theme::TEXT))
                        .line_clamp(3)
                        .child(body),
                ),
        )
}

fn empty_state(failed: bool) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .py(px(120.))
        .child(
            div()
                .text_3xl()
                .text_color(c(if failed { theme::DANGER } else { theme::HONEY }))
                .child("⬢"),
        )
        .child(
            div()
                .text_sm()
                .text_color(c(theme::MUTED))
                .child(if failed {
                    "The run did not finish. See the message above."
                } else {
                    "Pick a mode, enter a topic, and press Plan."
                }),
        )
}

fn badge(label: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .px_2()
        .py(px(1.))
        .rounded_md()
        .border_1()
        .border_color(color)
        .text_xs()
        .text_color(color)
        .child(label.into())
}

fn claims_view(r: &Report) -> impl IntoElement {
    let l = &r.ledger;
    let rows = l.ranked().into_iter().map(|i| {
        let cl = &l.claims[i];
        let conf = l.confidence(cl);
        let voices = l.voices(cl);
        v_flex()
            .gap_2()
            .p_3()
            .border_b_1()
            .border_color(c(theme::LINE))
            .child(div().text_sm().child(cl.text.clone()))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        div()
                            .w(px(80.))
                            .h(px(6.))
                            .rounded_full()
                            .bg(c(theme::RAISED))
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .w(relative(conf))
                                    .bg(c(theme::HONEY)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(c(theme::MUTED))
                            .child(format!("{:.0}%", conf * 100.)),
                    )
                    .child(badge(
                        format!(
                            "{voices} independent voice{}",
                            if voices == 1 { "" } else { "s" }
                        ),
                        c(theme::MUTED),
                    ))
                    .when(l.primary(cl), |d| {
                        d.child(badge("primary source", c(0x34d399)))
                    })
                    .when(!cl.contradicts.is_empty(), |d| {
                        d.child(badge("contested", c(theme::DANGER)))
                    })
                    .when_some(l.provenance(cl), |d, p| {
                        d.child(badge(format!("first seen {p}"), c(theme::MUTED)))
                    })
                    .when_some(cl.verdict.clone(), |d, v| {
                        d.child(badge(v, c(theme::HONEY)))
                    })
                    .child(
                        div().text_xs().text_color(c(theme::MUTED)).child(format!(
                            "sources {}",
                            cl.sources
                                .iter()
                                .map(|s| format!("[{}]", s + 1))
                                .collect::<Vec<_>>()
                                .join(" ")
                        )),
                    ),
            )
    });
    v_flex().children(rows).when(l.claims.is_empty(), |d| {
        d.child(
            div()
                .p_4()
                .text_sm()
                .text_color(c(theme::MUTED))
                .child("No claims recorded."),
        )
    })
}

fn sources_view(r: &Report) -> impl IntoElement {
    let rows = r.ledger.sources.iter().enumerate().map(|(i, s)| {
        h_flex()
            .gap_3()
            .p_3()
            .border_b_1()
            .border_color(c(theme::LINE))
            .child(
                div()
                    .w(px(32.))
                    .flex_none()
                    .text_xs()
                    .text_color(c(theme::MUTED))
                    .child(format!("[{}]", i + 1)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(if s.title.is_empty() {
                        s.url.clone()
                    } else {
                        s.title.clone()
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(c(theme::MUTED))
                            .truncate()
                            .child(s.url.clone()),
                    ),
            )
            .child(badge(s.stratum.label(), theme::stratum(s.stratum)))
            .child(badge(
                format!("quality {:.0}%", s.quality * 100.),
                c(theme::MUTED),
            ))
            .when(s.cluster != i, |d| {
                d.child(badge(
                    format!("copy of [{}]", s.cluster + 1),
                    c(theme::DANGER),
                ))
            })
    });
    v_flex().children(rows)
}

fn ghosts_view(r: &Report) -> impl IntoElement {
    let rows = r.ghosts.iter().map(|g| {
        v_flex()
            .gap_2()
            .p_3()
            .border_b_1()
            .border_color(c(theme::LINE))
            .child(div().text_sm().truncate().child(g.url.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(c(theme::MUTED))
                    .child(format!("archived {} · {}", g.captured, g.snapshot)),
            )
            .children(g.removed.iter().map(|p| {
                div()
                    .pl_3()
                    .border_l_2()
                    .border_color(theme::stratum(Stratum::Buried))
                    .text_sm()
                    .text_color(c(theme::TEXT))
                    .child(p.clone())
            }))
    });
    v_flex().children(rows).when(r.ghosts.is_empty(), |d| {
        d.child(div().p_4().text_sm().text_color(c(theme::MUTED)).child("No deleted passages found. Run in Hidden data mode to compare pages against their archived copies."))
    })
}

impl Drop for DeepHoney {
    fn drop(&mut self) {
        if let Some(task) = self.abort.take() {
            task.abort();
        }
    }
}
