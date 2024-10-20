use cushy::{
    widget::{MakeWidget, WidgetList},
    widgets::{
        layers::{OverlayLayer, Overlayable},
        menu::MenuItem,
        Menu,
    },
};

#[derive(Debug, Clone)]
enum MainMenuOptions {
    OpenVideo,
    OpenImage,
    Third,
    Fourth,
}

pub struct MainMenu(Menu<MainMenuOptions>);

impl MainMenu {
    pub fn new() -> Self {
        let menu = Menu::new()
            .on_selected(|selected| {})
            .with(MenuItem::new(MainMenuOptions::OpenVideo, "Open Video"))
            .with(MenuItem::new(MainMenuOptions::OpenImage, "Open Image"))
            .with(MenuItem::new(MainMenuOptions::Third, "Third"))
            .with(MenuItem::new(MainMenuOptions::Fourth, "Fourth"));

        Self(menu)
    }

    pub fn view(&self) -> impl MakeWidget {
        let overlay = OverlayLayer::default();

        "Menu"
            .into_button()
            .on_click({
                let overlay = overlay.clone();
                let menu = self.0.clone();
                move |click| {
                    if let Some(click) = click {
                        menu.overlay_in(&overlay).at(click.window_location).show();
                    }
                }
            })
            .and(overlay)
            .into_layers()
    }
}
