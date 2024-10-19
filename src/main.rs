use cushy::Run;

use gui_cushy::App;

fn main() -> cushy::Result {
    let app = App::default();
    app.view().run()
}
