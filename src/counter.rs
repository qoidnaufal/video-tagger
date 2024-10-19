use std::sync::{Arc, Mutex};

use cushy::{
    value::{Destination, Dynamic, IntoReader},
    widget::MakeWidget,
};

#[derive(Debug, Clone)]
pub struct Counter {
    value: i32,
}

// this just to imitate a regular api
impl Counter {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    pub fn inc(&mut self) {
        self.value += 1
    }

    pub fn dec(&mut self) {
        self.value -= 1
    }

    pub fn reset(&mut self) {
        self.value = 0
    }
}

// this is how to treat Counter as an app state
pub fn counter(state: Arc<Mutex<Counter>>) -> impl MakeWidget {
    let value = Dynamic::new(0);

    let inc = "+".into_button().on_click({
        let s = Arc::clone(&state);
        let value = value.clone();
        move |_| {
            let mut s = s.lock().unwrap();
            s.inc();
            value.set(s.value);
        }
    });

    let dec = "-".into_button().on_click({
        let s = Arc::clone(&state);
        let value = value.clone();
        move |_| {
            let mut s = s.lock().unwrap();
            s.dec();
            value.set(s.value);
        }
    });

    let reset = "reset".into_button().on_click({
        let s = Arc::clone(&state);
        let value = value.clone();
        move |_| {
            let mut s = s.lock().unwrap();
            s.reset();
            value.set(s.value);
        }
    });

    let buttons = dec.and(reset).and(inc).into_columns();

    value.into_label().and(buttons).into_rows()
}
