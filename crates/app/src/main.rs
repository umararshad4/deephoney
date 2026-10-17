mod settings;
mod theme;
mod ui;

use std::sync::Arc;

use gpui_kit::*;

actions!(deephoney, [Quit]);

fn main() {
    let rt = Arc::new(tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().expect("tokio runtime"));
    // Plug your research loop in here: any `engine::Researcher`.
    let researcher: Arc<dyn engine::Researcher> = Arc::new(engine::DeepResearch);

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        theme::apply(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::centered(None, size(px(1320.), px(860.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("DeepHoney".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(14.), px(14.))),
            }),
            window_min_size: Some(size(px(900.), px(600.))),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| ui::DeepHoney::new(rt, researcher, window, cx)))
            .expect("open window");
        cx.activate(true);
    });
}
