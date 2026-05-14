use std::{collections::VecDeque, sync::Arc};

use image::{DynamicImage, RgbaImage};
use ratatui::layout::Size;
use ratatui_image::{picker::Picker, protocol::Protocol, Resize};

use crate::{
    config::AppConfig,
    providers::{LlmProvider, ModelEvent, ModelRequest},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Normal,
    Input,
    Streaming,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug)]
pub enum AppEvent {
    Model(ModelEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    Insert(char),
    Backspace,
}

pub struct ProviderRequest {
    pub provider: Arc<dyn LlmProvider>,
    pub request: ModelRequest,
}

pub struct App {
    pub config: AppConfig,
    pub provider: Arc<dyn LlmProvider>,
    pub mode: UiMode,
    pub transcript: Vec<Message>,
    pub input: String,
    pub status: String,
    pub should_quit: bool,
    pub logo: Option<Protocol>,
}

impl App {
    pub fn new(config: AppConfig, provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            status: format!("Provider: {}", config.default_provider),
            config,
            provider,
            mode: UiMode::Input,
            transcript: Vec::new(),
            input: String::new(),
            should_quit: false,
            logo: build_logo_protocol(),
        }
    }

    pub fn edit_input(&mut self, action: InputAction) {
        if self.mode == UiMode::Streaming {
            return;
        }

        self.mode = UiMode::Input;
        match action {
            InputAction::Insert(ch) => self.input.push(ch),
            InputAction::Backspace => {
                self.input.pop();
            }
        }
    }

    pub fn submit_input(&mut self) -> Option<ProviderRequest> {
        if self.mode == UiMode::Streaming {
            return None;
        }

        let content = self.input.trim().to_owned();
        if content.is_empty() {
            return None;
        }

        self.input.clear();
        self.transcript.push(Message {
            role: Role::User,
            content: content.clone(),
        });
        self.transcript.push(Message {
            role: Role::Assistant,
            content: String::new(),
        });
        self.mode = UiMode::Streaming;
        self.status = "Streaming response".to_owned();

        Some(ProviderRequest {
            provider: Arc::clone(&self.provider),
            request: ModelRequest {
                messages: self.transcript.clone(),
            },
        })
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Model(ModelEvent::Token(token)) => self.append_assistant_token(&token),
            AppEvent::Model(ModelEvent::Done) => {
                self.mode = UiMode::Input;
                self.status = format!("Provider: {}", self.config.default_provider);
            }
            AppEvent::Model(ModelEvent::Error(error)) => {
                self.append_assistant_token(&format!("\nError: {error}"));
                self.mode = UiMode::Input;
                self.status = "Provider error".to_owned();
            }
        }
    }

    pub fn cancel_input(&mut self) {
        if self.mode != UiMode::Streaming {
            self.input.clear();
            self.mode = UiMode::Normal;
        }
    }

    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
    }

    fn append_assistant_token(&mut self, token: &str) {
        if let Some(message) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|message| message.role == Role::Assistant)
        {
            message.content.push_str(token);
        }
    }
}

impl From<ModelEvent> for AppEvent {
    fn from(value: ModelEvent) -> Self {
        Self::Model(value)
    }
}

fn build_logo_protocol() -> Option<Protocol> {
    let image = image::load_from_memory(include_bytes!("assets/artui.png")).ok()?;
    let image = crop_logo(image)?;
    let picker = Picker::halfblocks();
    picker
        .new_protocol(image, Size::new(18, 6), Resize::Fit(None))
        .ok()
}

fn crop_logo(image: DynamicImage) -> Option<DynamicImage> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for y in 0..height {
        for x in 0..width {
            let [red, green, blue, alpha] = rgba.get_pixel(x, y).0;
            if alpha > 12 && is_logo_pixel(red, green, blue) {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                found = true;
            }
        }
    }

    if !found {
        return None;
    }

    let padding = 18;
    min_x = min_x.saturating_sub(padding);
    min_y = min_y.saturating_sub(padding);
    max_x = (max_x + padding).min(width.saturating_sub(1));
    max_y = (max_y + padding).min(height.saturating_sub(1));

    let mut cropped = image::imageops::crop_imm(
        &rgba,
        min_x,
        min_y,
        max_x.saturating_sub(min_x) + 1,
        max_y.saturating_sub(min_y) + 1,
    )
    .to_image();
    remove_edge_background(&mut cropped);
    tint_logo(&mut cropped);
    Some(DynamicImage::ImageRgba8(cropped))
}

fn is_logo_pixel(red: u8, green: u8, blue: u8) -> bool {
    let blue_mark = blue > red.saturating_add(24) && blue > green.saturating_add(6);
    let dark_screen = red < 70 && green < 85 && blue < 115;
    blue_mark || dark_screen
}

fn remove_edge_background(image: &mut RgbaImage) {
    let (width, height) = image.dimensions();
    let mut visited = vec![false; width.saturating_mul(height) as usize];
    let mut queue = VecDeque::new();

    for x in 0..width {
        queue.push_back((x, 0));
        queue.push_back((x, height.saturating_sub(1)));
    }
    for y in 0..height {
        queue.push_back((0, y));
        queue.push_back((width.saturating_sub(1), y));
    }

    while let Some((x, y)) = queue.pop_front() {
        let index = (y * width + x) as usize;
        if visited[index] {
            continue;
        }
        visited[index] = true;

        let [red, green, blue, alpha] = image.get_pixel(x, y).0;
        if alpha > 12 && !is_background_pixel(red, green, blue) {
            continue;
        }

        image.get_pixel_mut(x, y).0[3] = 0;
        if x > 0 {
            queue.push_back((x - 1, y));
        }
        if x + 1 < width {
            queue.push_back((x + 1, y));
        }
        if y > 0 {
            queue.push_back((x, y - 1));
        }
        if y + 1 < height {
            queue.push_back((x, y + 1));
        }
    }
}

fn is_background_pixel(red: u8, green: u8, blue: u8) -> bool {
    let bright = red > 210 && green > 210 && blue > 210;
    let neutral = red.abs_diff(green) < 18 && green.abs_diff(blue) < 18 && red.abs_diff(blue) < 18;
    bright && neutral
}

fn tint_logo(image: &mut RgbaImage) {
    for pixel in image.pixels_mut() {
        let [red, green, blue, alpha] = pixel.0;
        if alpha == 0 {
            continue;
        }

        let luminance = (u16::from(red) * 30 + u16::from(green) * 59 + u16::from(blue) * 11) / 100;
        let strength = 72 + luminance.min(183);
        pixel.0 = [
            scale_channel(253, strength),
            scale_channel(151, strength),
            scale_channel(31, strength),
            alpha,
        ];
    }
}

fn scale_channel(channel: u8, strength: u16) -> u8 {
    ((u16::from(channel) * strength) / 255).min(255) as u8
}
