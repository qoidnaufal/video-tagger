#![allow(dead_code, unused_variables)]

mod counter;
mod video_player;

use std::{
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};

use ffmpeg_next as ffmpeg;

use cushy::kludgine::{
    image::ImageBuffer,
    wgpu::{FilterMode, TextureFormat, TextureUsages},
};
use cushy::kludgine::{image::ImageReader, AnyTexture};
use cushy::kludgine::{image::Rgb, LazyTexture};
use cushy::value::{Destination, Dynamic, Switchable};
use cushy::widget::{MakeWidget, SharedCallback};
use cushy::widgets::{layers::Modal, Image};
use cushy::WithClone;
use cushy::{figures::units::Lp, kludgine::image::DynamicImage};

use futures::{future::OptionFuture, Future, FutureExt};

use counter::Counter;
use video_player::{ControlCommand, VideoPlayer};

pub struct StreamClock {
    time_base_seconds: f64,
    start_time: std::time::Instant,
}

impl StreamClock {
    pub fn new(stream: &ffmpeg::format::stream::Stream) -> Self {
        let time_base_seconds = stream.time_base();
        let time_base_seconds =
            time_base_seconds.numerator() as f64 / time_base_seconds.denominator() as f64;
        let start_time = std::time::Instant::now();

        Self {
            time_base_seconds,
            start_time,
        }
    }

    pub fn convert_pts_to_instant(&self, pts: Option<i64>) -> Option<std::time::Duration> {
        pts.and_then(|pts| {
            let pts_since_start =
                std::time::Duration::from_secs_f64(pts as f64 * self.time_base_seconds);

            self.start_time.checked_add(pts_since_start)
        })
        .map(|absolute_pts| absolute_pts.duration_since(std::time::Instant::now()))
    }
}

pub fn yield_now() -> YieldNow {
    YieldNow(false)
}

pub struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        if !self.0 {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

struct VideoDecoder {
    control_sender: std::sync::mpsc::Sender<ControlCommand>,
    packet_sender: std::sync::mpsc::SyncSender<ffmpeg::codec::packet::Packet>,
    receiver_thread: Option<std::thread::JoinHandle<()>>,
}

impl VideoDecoder {
    fn start(
        stream: &ffmpeg::format::stream::Stream,
        mut frame_callback: Box<dyn FnMut(&ffmpeg::util::frame::Video) + Send>,
    ) -> Self {
        let (control_sender, control_receiver) = std::sync::mpsc::channel::<ControlCommand>();
        let (packet_sender, packet_receiver) = std::sync::mpsc::sync_channel(128);

        let decoder_ctx = ffmpeg::codec::Context::from_parameters(stream.parameters()).unwrap();
        let mut packet_decoder = decoder_ctx.decoder().video().unwrap();

        let clock = StreamClock::new(stream);

        let receiver_thread = std::thread::Builder::new()
            .name("Receiver Thread".into())
            .spawn(move || {
                futures::executor::block_on(async move {
                    let packet_receiver_impl = async {
                        loop {
                            let Ok(packet) = packet_receiver.recv() else {
                                break;
                            };

                            yield_now().await;

                            packet_decoder.send_packet(&packet).unwrap();
                            let mut decoded_frame = ffmpeg::util::frame::Video::empty();

                            while packet_decoder.receive_frame(&mut decoded_frame).is_ok() {
                                if let Some(delay) =
                                    clock.convert_pts_to_instant(decoded_frame.pts())
                                {
                                    std::thread::sleep(delay)
                                }

                                frame_callback(&decoded_frame);
                            }
                        }
                    }
                    .fuse()
                    .shared();

                    let playing = true;

                    loop {
                        let packet_receiver: OptionFuture<_> = if playing {
                            Some(packet_receiver_impl.clone())
                        } else {
                            None
                        }
                        .into();

                        futures::pin_mut!(packet_receiver);

                        futures::select! {
                            _ = packet_receiver => {}
                        }
                    }
                })
            })
            .unwrap();

        Self {
            control_sender,
            packet_sender,
            receiver_thread: Some(receiver_thread),
        }
    }

    pub fn receive_packet(&self, packet: ffmpeg::codec::packet::packet::Packet) -> bool {
        match self.packet_sender.send(packet) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    pub fn send_control_message(&self, message: ControlCommand) {
        self.control_sender.send(message).unwrap();
    }
}

pub struct App {
    image_source: Dynamic<Option<PathBuf>>,
    video_source: Dynamic<Option<PathBuf>>,
    counter: Arc<Mutex<Counter>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            image_source: Dynamic::new(None),
            video_source: Dynamic::new(None),
            counter: Arc::new(Mutex::new(Counter::new())),
        }
    }
}

impl App {
    fn handle_video_source(&self) -> impl MakeWidget {
        self.video_source.clone().switcher(move |source, _| {
            if let Some(source) = source {
                let path = source.clone();

                VideoPlayer::new()
                    .start(move |content| {
                        let path = path.clone();
                        futures::executor::block_on(async move {
                            let mut ictx = ffmpeg::format::input(&path).unwrap();
                            let stream = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
                            let vs_idx = stream.index();

                            let video_decoder = VideoDecoder::start(
                                &stream,
                                Box::new(move |yuv_frame| {
                                    let mut rgb_frame = ffmpeg::util::frame::Video::empty();
                                    let mut rescaler = rescaler(yuv_frame);
                                    rescaler.0.run(yuv_frame, &mut rgb_frame).unwrap();

                                    // do something with the rgb_frame
                                    let mut pixel_buffer = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(
                                        rgb_frame.width(),
                                        rgb_frame.height(),
                                    );

                                    let pixel_line_iter = pixel_buffer.chunks_mut(
                                        rgb_frame.width() as usize * std::mem::size_of::<Rgb<u8>>(),
                                    );

                                    let source_line_iter =
                                        rgb_frame.data(0).chunks_exact(rgb_frame.stride(0));

                                    for (source, dest) in source_line_iter.zip(pixel_line_iter) {
                                        dest.copy_from_slice(&source[..dest.len()]);
                                    }

                                    let pixel_buffer =
                                        DynamicImage::from(pixel_buffer).into_rgba8();

                                    let texture = LazyTexture::from_data(
                                        cushy::figures::Size::new(
                                            pixel_buffer.width().into(),
                                            pixel_buffer.height().into(),
                                        ),
                                        TextureFormat::Rgba8UnormSrgb,
                                        TextureUsages::TEXTURE_BINDING,
                                        FilterMode::Nearest,
                                        pixel_buffer.into_raw(),
                                    );
                                    let texture = AnyTexture::from(texture);

                                    content.set(texture);
                                }),
                            );

                            let playing = true;

                            let packet_forwarder_impl = async {
                                for (stream, packet) in ictx.packets() {
                                    if stream.index() == vs_idx {
                                        video_decoder.receive_packet(packet);
                                    }
                                }
                            }
                            .fuse()
                            .shared();

                            loop {
                                let packet_forwarder: OptionFuture<_> = if playing {
                                    Some(packet_forwarder_impl.clone())
                                } else {
                                    None
                                }
                                .into();

                                futures::pin_mut!(packet_forwarder);

                                futures::select! {
                                    _ = packet_forwarder => {}
                                }
                            }
                        })
                    })
                    .make_widget()
            } else {
                VideoPlayer::new().make_widget()
            }
        })
    }

    fn handle_image_source(&self, on_error: SharedCallback<String>) -> impl MakeWidget {
        self.image_source
            .clone()
            .switcher(move |source, _| {
                (source, &on_error).with_clone(|(source, on_error)| {
                    if let Some(source) = source {
                        match ImageReader::open(source).unwrap().decode() {
                            Ok(dyn_image) => {
                                let lazy_texture =
                                    LazyTexture::from_image(dyn_image, FilterMode::Nearest);
                                Image::new(lazy_texture).aspect_fit().make_widget()
                            }
                            Err(err) => {
                                on_error.invoke(format!("{err}"));
                                "No picture".make_widget()
                            }
                        }
                    } else {
                        "No picture".make_widget()
                    }
                })
            })
            .centered()
            .pad_by(Lp::new(10))
    }

    pub fn view(&self) -> impl MakeWidget {
        let image_source = self.image_source.clone();
        let open_image_button = file_picker("open image", image_source);

        let video_source = self.video_source.clone();
        let open_video_button = file_picker("open video", video_source);

        let modal = Modal::new();
        let on_error = error_callback(modal.clone());

        let image = self.handle_image_source(on_error);
        let video = self.handle_video_source();

        let counter = self.counter.clone();
        let counter = counter::counter(counter);

        open_image_button
            .and(open_video_button)
            .into_rows()
            .and(counter)
            .into_columns()
            .and(image)
            .and(video)
            .into_rows()
            .and(modal)
            .into_layers()
    }
}

fn error_callback(modal: Modal) -> SharedCallback<String> {
    SharedCallback::new({
        move |err: String| {
            modal.present(
                err.and("OK".into_button().on_click({
                    let modal = modal.clone();
                    move |_| {
                        modal.dismiss();
                    }
                }))
                .into_rows()
                .contain(),
            );
        }
    })
}

fn file_picker(label: &str, source: Dynamic<Option<PathBuf>>) -> impl MakeWidget {
    label.into_button().on_click(move |_| {
        let source = source.clone();
        std::thread::Builder::new()
            .name("File Picker Thread".into())
            .spawn(move || {
                let pick_file = rfd::FileDialog::new().pick_file();
                if let Some(path) = pick_file {
                    source.set(Some(path));
                }
            })
            .unwrap();
    })
}

pub struct Rescaler(ffmpeg::software::scaling::Context);

unsafe impl std::marker::Send for Rescaler {}

fn rescaler(frame: &ffmpeg::util::frame::Video) -> Rescaler {
    Rescaler(
        ffmpeg::software::scaling::Context::get(
            frame.format(),
            frame.width(),
            frame.height(),
            ffmpeg::format::Pixel::RGB24,
            frame.width(),
            frame.height(),
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .unwrap(),
    )
}
