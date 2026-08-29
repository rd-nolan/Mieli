pub struct Mieli;

impl Mieli {
    pub fn new(_: &mut gpui::Context<Self>) -> Self {
        Self
    }
}

impl gpui::Render for Mieli {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}
