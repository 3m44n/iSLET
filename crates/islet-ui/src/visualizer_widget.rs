//! Frame-clock-synced Cairo spectrum bar visualizer (`≤ 60 FPS`, zero timer wakeups when paused).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;

const MIN_FRAME_INTERVAL_US: i64 = 16_000; // Cap interpolation redraws to ≤60 FPS

/// Smoothly interpolated 12–24 bar audio visualizer widget.
#[derive(Clone)]
pub struct SpectrumWidget {
    drawing_area: gtk4::DrawingArea,
    target_bars: Rc<RefCell<Vec<f32>>>,
    current_bars: Rc<RefCell<Vec<f32>>>,
    accent_rgb: Rc<Cell<(f64, f64, f64)>>,
    active: Rc<Cell<bool>>,
    tick_id: Rc<RefCell<Option<gtk4::TickCallbackId>>>,
}

impl SpectrumWidget {
    #[must_use]
    pub fn new(initial_bars: usize) -> Self {
        let bars = initial_bars.clamp(12, 24);
        let target_bars = Rc::new(RefCell::new(vec![0.0_f32; bars]));
        let current_bars = Rc::new(RefCell::new(vec![0.0_f32; bars]));
        // Default Catppuccin Blue (`#89b4fa`)
        let accent_rgb = Rc::new(Cell::new((0.537, 0.706, 0.980)));
        let active = Rc::new(Cell::new(false));
        let tick_id = Rc::new(RefCell::new(None));

        let drawing_area = gtk4::DrawingArea::builder()
            .content_width(68)
            .content_height(28)
            .valign(gtk4::Align::Center)
            .halign(gtk4::Align::End)
            .css_classes(["islet-spectrum"])
            .build();

        let current_for_draw = Rc::clone(&current_bars);
        let accent_for_draw = Rc::clone(&accent_rgb);

        drawing_area.set_draw_func(move |_, cr, width, height| {
            let values = current_for_draw.borrow();
            let count = values.len().max(1);
            let w = f64::from(width);
            let h = f64::from(height);

            let gap = 2.0;
            let total_gaps = gap * ((count.saturating_sub(1)) as f64);
            let bar_w = ((w - total_gaps) / (count as f64)).max(2.0);
            let (r, g, b) = accent_for_draw.get();

            for (i, &val) in values.iter().enumerate() {
                let x = (i as f64) * (bar_w + gap);
                let normalized = f64::from(val.clamp(0.08, 1.0));
                let bar_h = (normalized * (h - 2.0)).max(3.0);
                let y = (h - bar_h) * 0.5;
                let radius = (bar_w * 0.5).min(bar_h * 0.5);

                // Draw vertical rounded capsule bar
                cr.new_sub_path();
                cr.arc(
                    x + radius,
                    y + radius,
                    radius,
                    std::f64::consts::PI,
                    0.0,
                );
                cr.arc(
                    x + radius,
                    y + bar_h - radius,
                    radius,
                    0.0,
                    std::f64::consts::PI,
                );
                cr.close_path();

                cr.set_source_rgba(r, g, b, 0.35 + 0.65 * normalized);
                let _ = cr.fill();
            }
        });

        Self {
            drawing_area,
            target_bars,
            current_bars,
            accent_rgb,
            active,
            tick_id,
        }
    }

    #[must_use]
    pub fn widget(&self) -> &gtk4::DrawingArea {
        &self.drawing_area
    }

    /// Updates target FFT bar heights received from `islet-sources`.
    pub fn set_target_frame(&self, frame: &[f32]) {
        if frame.is_empty() {
            return;
        }
        let mut target = self.target_bars.borrow_mut();
        if target.len() != frame.len() {
            *target = frame.to_vec();
            self.current_bars.borrow_mut().resize(frame.len(), 0.0);
        } else {
            target.copy_from_slice(frame);
        }
    }

    /// Updates the visualizer bar color to match the album art's dominant RGB accent.
    pub fn set_accent_color(&self, rgb: Option<(u8, u8, u8)>) {
        let (r, g, b) = rgb.unwrap_or((137, 180, 250));
        self.accent_rgb.set((
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
        ));
        self.drawing_area.queue_draw();
    }

    /// Starts or stops the GTK4 frame-clock tick callback (`≤ 60 FPS`).
    ///
    /// When `should_run == false`, removes the tick callback completely so idle CPU is 0%.
    pub fn set_running(&self, should_run: bool) {
        if self.active.get() == should_run {
            return;
        }
        self.active.set(should_run);

        if should_run {
            let target_ref = Rc::clone(&self.target_bars);
            let current_ref = Rc::clone(&self.current_bars);
            let last_frame_us = Cell::new(0_i64);

            let id = self.drawing_area.add_tick_callback(move |area, clock| {
                let now_us = clock.frame_time();
                if now_us - last_frame_us.get() < MIN_FRAME_INTERVAL_US {
                    return glib::ControlFlow::Continue;
                }
                last_frame_us.set(now_us);

                let targets = target_ref.borrow();
                let mut currents = current_ref.borrow_mut();
                if currents.len() != targets.len() {
                    currents.resize(targets.len(), 0.0);
                }

                let mut changed = false;
                for (cur, &tgt) in currents.iter_mut().zip(targets.iter()) {
                    // Fast attack (0.38) for snappy beats, smooth exponential decay (0.20)
                    let alpha = if tgt > *cur { 0.38 } else { 0.20 };
                    let next = *cur + (tgt - *cur) * alpha;
                    if (next - *cur).abs() > 0.002 {
                        *cur = next;
                        changed = true;
                    }
                }

                if changed {
                    area.queue_draw();
                }
                glib::ControlFlow::Continue
            });
            *self.tick_id.borrow_mut() = Some(id);
        } else {
            if let Some(id) = self.tick_id.borrow_mut().take() {
                id.remove();
            }
            self.current_bars.borrow_mut().fill(0.0);
            self.target_bars.borrow_mut().fill(0.0);
            self.drawing_area.queue_draw();
        }
    }
}
