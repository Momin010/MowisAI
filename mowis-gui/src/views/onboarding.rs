/// Beautiful onboarding experience with animations
use crate::animation::{FadeAnimation, SlideAnimation, SpinnerAnimation};
use crate::theme::Theme;
use egui::{Color32, ColorImage, Frame, Pos2, Rect, RichText, Stroke, TextureHandle, Vec2};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnboardingStep {
    Welcome,
    SetupEngine,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnboardingAction {
    Continue,  // User clicked Next on welcome
    Skip,      // User skipped setup
    Complete,  // Setup complete
}

pub struct OnboardingView {
    step: OnboardingStep,
    fade_in: FadeAnimation,
    slide_in: SlideAnimation,
    spinner: SpinnerAnimation,
    progress: f32,
    progress_message: String,
    setup_failed: bool,
    welcome_svg_texture: Option<TextureHandle>,
}

impl OnboardingView {
    pub fn new() -> Self {
        Self {
            step: OnboardingStep::Welcome,
            fade_in: FadeAnimation::fade_in(Duration::from_millis(800)),
            slide_in: SlideAnimation::new(Duration::from_millis(600), 50.0, 0.0),
            spinner: SpinnerAnimation::new(),
            progress: 0.0,
            progress_message: String::new(),
            setup_failed: false,
            welcome_svg_texture: None,
        }
    }

    pub fn set_step(&mut self, step: OnboardingStep) {
        if self.step != step {
            self.step = step;
            self.fade_in = FadeAnimation::fade_in(Duration::from_millis(400));
            self.slide_in = SlideAnimation::new(Duration::from_millis(400), 30.0, 0.0);
        }
    }

    pub fn set_progress(&mut self, progress: f32, message: String) {
        self.progress = progress;
        self.progress_message = message;
    }
    
    pub fn set_failed(&mut self) {
        self.setup_failed = true;
    }
    
    pub fn is_on_setup_screen(&self) -> bool {
        self.step == OnboardingStep::SetupEngine
    }
}

impl Default for OnboardingView {
    fn default() -> Self {
        Self::new()
    }
}

pub fn show(
    view: &mut OnboardingView,
    ctx: &egui::Context,
    daemon_running: bool,
    daemon_setup_complete: bool,
) -> Option<OnboardingAction> {
    // Request repaint for animations
    ctx.request_repaint();

    let mut action = None;

    egui::CentralPanel::default()
        .frame(Frame::none().fill(Color32::BLACK))
        .show(ctx, |ui| {
            match view.step {
                OnboardingStep::Welcome => {
                    action = show_welcome(ui, view);
                }
                OnboardingStep::SetupEngine => {
                    let fade = view.fade_in.value();
                    let slide = view.slide_in.value();
                    action = show_setup_engine(ui, view, fade, slide, daemon_running, daemon_setup_complete);
                }
                OnboardingStep::Ready => {
                    let fade = view.fade_in.value();
                    let slide = view.slide_in.value();
                    action = show_ready(ui, fade, slide);
                }
            }
        });

    action
}

fn show_welcome(ui: &mut egui::Ui, view: &mut OnboardingView) -> Option<OnboardingAction> {
    let available = ui.available_size();
    
    // Pure black background
    let rect = Rect::from_min_size(Pos2::ZERO, available);
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    
    // Load SVG texture if not already loaded
    if view.welcome_svg_texture.is_none() {
        if let Ok(svg_data) = std::fs::read_to_string("Group.svg") {
            if let Ok(image) = load_svg_as_image(&svg_data, 840.0, 158.0) {
                view.welcome_svg_texture = Some(ui.ctx().load_texture(
                    "welcome_svg",
                    image,
                    Default::default()
                ));
            }
        }
    }
    
    // Center the SVG vertically
    let svg_height = 158.0;
    let text_y = (available.y - svg_height) * 0.5;
    ui.add_space(text_y);
    
    // Center horizontally and show the SVG
    ui.vertical_centered(|ui| {
        if let Some(texture) = &view.welcome_svg_texture {
            ui.image((texture.id(), Vec2::new(840.0, 158.0)));
        } else {
            // Fallback to text if SVG fails to load
            ui.label(
                RichText::new("Welcome to MowisAI")
                    .font(egui::FontId::proportional(64.0))
                    .color(Color32::WHITE)
            );
        }
    });
    
    // Position the continue button at bottom right
    let button_size = Vec2::new(120.0, 45.0);
    let button_margin = 40.0;
    let button_pos = Pos2::new(
        available.x - button_size.x - button_margin,
        available.y - button_size.y - button_margin
    );
    
    let button_rect = Rect::from_min_size(button_pos, button_size);
    
    // Draw the button
    let button_response = ui.allocate_rect(button_rect, egui::Sense::click());
    
    let button_color = if button_response.hovered() {
        Color32::from_rgb(50, 150, 230) // Lighter blue on hover
    } else {
        Color32::from_rgb(30, 120, 200) // Blue color
    };
    
    ui.painter().rect_filled(button_rect, 22.0, button_color);
    
    // Draw button text
    ui.painter().text(
        button_rect.center(),
        egui::Align2::CENTER_CENTER,
        "continue",
        egui::FontId::proportional(18.0),
        Color32::WHITE,
    );
    
    if button_response.clicked() {
        return Some(OnboardingAction::Continue);
    }
    
    None
}

/// Load SVG and convert to ColorImage
fn load_svg_as_image(svg_data: &str, width: f32, height: f32) -> Result<ColorImage, Box<dyn std::error::Error>> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg_data, &opt)?;
    
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width as u32, height as u32)
        .ok_or("Failed to create pixmap")?;
    
    resvg::render(
        &tree,
        usvg::Transform::from_scale(
            width / tree.size().width(),
            height / tree.size().height()
        ),
        &mut pixmap.as_mut(),
    );
    
    let pixels = pixmap.data();
    let size = [pixmap.width() as usize, pixmap.height() as usize];
    
    Ok(ColorImage::from_rgba_unmultiplied(size, pixels))
}

fn show_setup_engine(ui: &mut egui::Ui, view: &mut OnboardingView, fade: f32, slide: f32, _daemon_running: bool, daemon_setup_complete: bool) -> Option<OnboardingAction> {
    let available = ui.available_size();
    let content_h = 350.0;
    let top_pad = ((available.y - content_h) * 0.5).max(80.0);
    ui.add_space(top_pad + slide);

    let alpha = (fade * 255.0) as u8;

    ui.vertical_centered(|ui| {
        // Title
        ui.label(
            RichText::new("Setting up AI Engine")
                .font(egui::FontId::proportional(32.0))
                .color(Color32::from_rgba_premultiplied(250, 248, 240, alpha))
                .strong(),
        );

        ui.add_space(16.0);

        // Show different content based on backend state
        if view.setup_failed {
            // Backend failed
            ui.label(
                RichText::new("⚠")
                    .font(egui::FontId::proportional(64.0))
                    .color(Color32::from_rgba_premultiplied(255, 100, 100, alpha)),
            );
            
            ui.add_space(16.0);
            
            ui.label(
                RichText::new("Backend failed to start")
                    .font(Theme::font_body())
                    .color(Color32::from_rgba_premultiplied(200, 198, 190, alpha)),
            );
            
            ui.add_space(8.0);
            
            ui.label(
                RichText::new("You can continue without the backend or try again later")
                    .font(Theme::font_label())
                    .color(Color32::from_rgba_premultiplied(120, 118, 110, alpha)),
            );
            
            ui.add_space(32.0);
            
            if fade > 0.7 {
                let btn_alpha = ((fade - 0.7) * 3.33 * 255.0) as u8;
                
                let button = egui::Button::new(
                    RichText::new("Continue Anyway →")
                        .font(egui::FontId::proportional(16.0))
                        .color(Color32::from_rgba_premultiplied(0, 0, 0, btn_alpha))
                        .strong(),
                )
                .fill(Color32::from_rgba_premultiplied(255, 255, 255, btn_alpha))
                .rounding(8.0)
                .min_size(Vec2::new(180.0, 48.0));

                if ui.add(button).clicked() {
                    return Some(OnboardingAction::Skip);
                }
                None
            } else {
                None
            }
        } else if daemon_setup_complete {
            // Backend ready!
            ui.label(
                RichText::new("✓")
                    .font(egui::FontId::proportional(64.0))
                    .color(Color32::from_rgba_premultiplied(64, 224, 208, alpha)),
            );
            
            ui.add_space(16.0);
            
            ui.label(
                RichText::new("AI Engine is ready!")
                    .font(Theme::font_body())
                    .color(Color32::from_rgba_premultiplied(200, 198, 190, alpha)),
            );
            
            ui.add_space(32.0);
            
            if fade > 0.7 {
                let btn_alpha = ((fade - 0.7) * 3.33 * 255.0) as u8;
                
                let button = egui::Button::new(
                    RichText::new("Continue →")
                        .font(egui::FontId::proportional(16.0))
                        .color(Color32::from_rgba_premultiplied(0, 0, 0, btn_alpha))
                        .strong(),
                )
                .fill(Color32::from_rgba_premultiplied(64, 224, 208, btn_alpha))
                .rounding(8.0)
                .min_size(Vec2::new(160.0, 48.0));

                if ui.add(button).clicked() {
                    return Some(OnboardingAction::Complete);
                }
                None
            } else {
                None
            }
        } else {
            // Still loading
            // Animated spinner
            let spinner_size = 64.0;
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(spinner_size, spinner_size),
                egui::Sense::hover(),
            );
            
            let painter = ui.painter();
            let center = rect.center();
            let rotation = view.spinner.rotation();
            
            // Draw spinning arc in turquoise
            for i in 0..8 {
                let angle = rotation + (i as f32 * std::f32::consts::TAU / 8.0);
                let start = center + Vec2::new(angle.cos(), angle.sin()) * (spinner_size * 0.3);
                let end = center + Vec2::new(angle.cos(), angle.sin()) * (spinner_size * 0.45);
                let alpha_i = ((1.0 - i as f32 / 8.0) * fade * 255.0) as u8;
                painter.line_segment(
                    [start, end],
                    Stroke::new(3.0, Color32::from_rgba_premultiplied(64, 224, 208, alpha_i)),
                );
            }

            ui.add_space(24.0);

            // Progress bar
            Frame::none()
                .fill(Color32::from_rgba_premultiplied(255, 255, 255, (fade * 15.0) as u8))
                .rounding(Theme::ROUNDING_PILL)
                .inner_margin(egui::Margin::same(4.0))
                .show(ui, |ui| {
                    ui.set_width(400.0);
                    ui.set_height(8.0);
                    
                    let progress_width = 392.0 * view.progress;
                    let progress_rect = Rect::from_min_size(
                        ui.min_rect().min,
                        Vec2::new(progress_width, 8.0),
                    );
                    
                    ui.painter().rect_filled(
                        progress_rect,
                        Theme::ROUNDING_PILL,
                        Color32::from_rgba_premultiplied(64, 224, 208, alpha),
                    );
                });

            ui.add_space(16.0);

            // Progress message
            ui.label(
                RichText::new(&view.progress_message)
                    .font(Theme::font_body())
                    .color(Color32::from_rgba_premultiplied(200, 198, 190, alpha)),
            );

            ui.add_space(8.0);

            // Percentage
            ui.label(
                RichText::new(format!("{}%", (view.progress * 100.0) as u32))
                    .font(Theme::font_label())
                    .color(Color32::from_rgba_premultiplied(120, 118, 110, alpha)),
            );

            ui.add_space(32.0);

            // Skip button
            if fade > 0.7 {
                let btn_alpha = ((fade - 0.7) * 3.33 * 255.0) as u8;
                
                ui.label(
                    RichText::new("Don't have QEMU or WSL2 installed?")
                        .font(Theme::font_label())
                        .color(Color32::from_rgba_premultiplied(120, 118, 110, btn_alpha)),
                );
                
                ui.add_space(8.0);
                
                let button = egui::Button::new(
                    RichText::new("Skip for now →")
                        .font(egui::FontId::proportional(14.0))
                        .color(Color32::from_rgba_premultiplied(250, 248, 240, btn_alpha))
                        .strong(),
                )
                .fill(Color32::from_rgba_premultiplied(255, 255, 255, (fade * 20.0) as u8))
                .stroke(Stroke::new(1.0, Color32::from_rgba_premultiplied(64, 224, 208, btn_alpha)))
                .rounding(Theme::ROUNDING_PILL)
                .min_size(Vec2::new(160.0, 40.0));

                if ui.add(button).clicked() {
                    return Some(OnboardingAction::Skip);
                }
                None
            } else {
                None
            }
        }
    });

    None
}

fn show_ready(ui: &mut egui::Ui, fade: f32, slide: f32) -> Option<OnboardingAction> {
    let available = ui.available_size();
    let content_h = 250.0;
    let top_pad = ((available.y - content_h) * 0.5).max(100.0);
    ui.add_space(top_pad + slide);

    let alpha = (fade * 255.0) as u8;

    ui.vertical_centered(|ui| {
        // Success checkmark with scale animation in turquoise
        let scale = 0.8 + fade * 0.2;
        ui.label(
            RichText::new("✓")
                .font(egui::FontId::proportional(80.0 * scale))
                .color(Color32::from_rgba_premultiplied(64, 224, 208, alpha)),
        );

        ui.add_space(24.0);

        ui.label(
            RichText::new("Ready to Go!")
                .font(egui::FontId::proportional(36.0))
                .color(Color32::from_rgba_premultiplied(250, 248, 240, alpha))
                .strong(),
        );

        ui.add_space(16.0);

        ui.label(
            RichText::new("Your AI engine is running and ready")
                .font(Theme::font_body())
                .color(Color32::from_rgba_premultiplied(200, 198, 190, alpha)),
        );

        ui.add_space(48.0);

        // Continue button in turquoise
        if fade > 0.7 {
            let btn_alpha = ((fade - 0.7) * 3.33 * 255.0) as u8;
            let button = egui::Button::new(
                RichText::new("Start Building →")
                    .font(egui::FontId::proportional(16.0))
                    .color(Color32::from_rgba_premultiplied(10, 10, 10, btn_alpha))
                    .strong(),
            )
            .fill(Color32::from_rgba_premultiplied(64, 224, 208, btn_alpha))
            .rounding(Theme::ROUNDING_PILL)
            .min_size(Vec2::new(200.0, 48.0));

            if ui.add(button).clicked() {
                return Some(OnboardingAction::Complete);
            }
            None
        } else {
            None
        }
    });

    None
}
