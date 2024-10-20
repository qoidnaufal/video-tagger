use std::thread::JoinHandle;

use cushy::animation::ZeroToOne;
use cushy::context::{GraphicsContext, LayoutContext};
use cushy::figures::units::{Px, UPx};
use cushy::figures::{FloatConversion, IntoSigned, IntoUnsigned, Point, Rect, Size, Zero};
use cushy::kludgine::image::DynamicImage;
use cushy::kludgine::wgpu::FilterMode;
use cushy::kludgine::{AnyTexture, LazyTexture};
use cushy::value::{Dynamic, IntoValue, Source, Value};
use cushy::widget::Widget;
use cushy::widgets::image::{Aspect, ImageScaling};
use cushy::ConstraintLimit;

#[derive(Debug)]
pub enum ControlCommand {
    Play,
    Pause,
    Stop,
}

#[derive(Debug)]
pub struct VideoPlayer {
    contents: Dynamic<AnyTexture>,
    scaling: Value<ImageScaling>,
    playback_thread: Option<JoinHandle<()>>,
    control_sender: Option<std::sync::mpsc::Sender<ControlCommand>>,
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        if let Some(handle) = self.playback_thread.take() {
            drop(handle)
        }
    }
}

impl VideoPlayer {
    pub fn new() -> Self {
        let dyn_image = DynamicImage::new_rgb8(500, 300);
        let lazy_texture = LazyTexture::from_image(dyn_image, FilterMode::Nearest);
        let contents = Dynamic::new(AnyTexture::Lazy(lazy_texture));

        let scaling = ImageScaling::Aspect {
            mode: Aspect::Fit,
            orientation: Size::ZERO,
        }
        .into_value();

        Self {
            contents,
            scaling,
            playback_thread: None,
            control_sender: None,
        }
    }

    pub fn start<F>(&mut self, playback: F)
    where
        F: FnOnce(Dynamic<AnyTexture>) + Send + Sync + 'static,
    {
        let contents = self.contents.clone();
        let texture = contents.clone();
        let playback_thread = Some(
            std::thread::Builder::new()
                .name("Playback Thread".into())
                .spawn(|| playback(texture))
                .unwrap(),
        );

        self.contents = contents;
        self.playback_thread = playback_thread;
        self.control_sender = None;
    }

    fn calculate_frame_rect(
        &self,
        texture: &AnyTexture,
        within_size: Size<UPx>,
        context: &mut GraphicsContext<'_, '_, '_, '_>,
    ) -> Rect<Px> {
        let within_size = within_size.into_signed();
        let size = texture.size().into_signed();

        match self.scaling.get_tracking_invalidate(context) {
            ImageScaling::Aspect { mode, orientation } => {
                let scale_width = within_size.width.into_float() / size.width.into_float();
                let scale_height = within_size.height.into_float() / size.height.into_float();

                let effective_scale = match mode {
                    Aspect::Fill => scale_width.max(scale_height),
                    Aspect::Fit => scale_width.min(scale_height),
                };
                let scaled = size * effective_scale;

                let x = (within_size.width - scaled.width) * *orientation.width;
                let y = (within_size.height - scaled.height) * *orientation.height;

                Rect::new(Point::new(x, y), scaled)
            }
            ImageScaling::Stretch => within_size.into(),
            ImageScaling::Scale(factor) => {
                let size = size.map(|px| px * factor);
                size.into()
            }
        }
    }
}

impl Widget for VideoPlayer {
    fn redraw(&mut self, context: &mut GraphicsContext<'_, '_, '_, '_>) {
        use cushy::context::Trackable;

        self.contents.redraw_when_changed(context);

        self.contents.map_ref(|texture| {
            let rect = self.calculate_frame_rect(texture, context.gfx.size(), context);
            context.gfx.draw_texture(texture, rect, ZeroToOne::new(1.));
        });
    }

    fn layout(
        &mut self,
        available_space: Size<ConstraintLimit>,
        context: &mut LayoutContext<'_, '_, '_, '_>,
    ) -> cushy::figures::Size<cushy::figures::units::UPx> {
        let rect = self.contents.map_ref(|texture| {
            self.calculate_frame_rect(texture, available_space.map(ConstraintLimit::max), context)
        });
        rect.size.into_unsigned()
    }
}
