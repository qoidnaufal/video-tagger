use cushy::{value::Dynamic, widget::MakeWidget, Run};

use gui_cushy::App;

fn main() -> cushy::Result {
    let app = App::default();
    app.view().into_window().maximized(Dynamic::new(true)).run()
}
