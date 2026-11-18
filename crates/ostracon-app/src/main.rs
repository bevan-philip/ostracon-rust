use gpui_kit::*;

struct Shell;

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("Ostracon")
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Shell))
                .expect("failed to open window");
        });
}
