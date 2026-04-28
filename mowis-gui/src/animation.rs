/// Animation utilities for smooth UI transitions
use std::time::{Duration, Instant};

/// Easing functions for smooth animations
pub struct Easing;

impl Easing {
    /// Ease out cubic - decelerating to zero velocity
    pub fn ease_out_cubic(t: f32) -> f32 {
        let t = t - 1.0;
        t * t * t + 1.0
    }

    /// Ease in out cubic - acceleration until halfway, then deceleration
    pub fn ease_in_out_cubic(t: f32) -> f32 {
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            let t = t - 1.0;
            1.0 + 4.0 * t * t * t
        }
    }

    /// Ease out back - overshoots then settles
    pub fn ease_out_back(t: f32) -> f32 {
        let c1 = 1.70158;
        let c3 = c1 + 1.0;
        1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
    }

    /// Elastic ease out - bouncy effect
    pub fn ease_out_elastic(t: f32) -> f32 {
        let c4 = (2.0 * std::f32::consts::PI) / 3.0;
        if t == 0.0 {
            0.0
        } else if t == 1.0 {
            1.0
        } else {
            2.0_f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
        }
    }
}

/// Animation state tracker
#[derive(Clone)]
pub struct Animation {
    start_time: Instant,
    duration: Duration,
    completed: bool,
}

impl Animation {
    /// Create a new animation
    pub fn new(duration: Duration) -> Self {
        Self {
            start_time: Instant::now(),
            duration,
            completed: false,
        }
    }

    /// Get progress (0.0 to 1.0)
    pub fn progress(&mut self) -> f32 {
        let elapsed = self.start_time.elapsed();
        if elapsed >= self.duration {
            self.completed = true;
            1.0
        } else {
            elapsed.as_secs_f32() / self.duration.as_secs_f32()
        }
    }

    /// Check if animation is complete
    pub fn is_complete(&self) -> bool {
        self.completed
    }

    /// Reset animation
    pub fn reset(&mut self) {
        self.start_time = Instant::now();
        self.completed = false;
    }
}

/// Fade animation helper
pub struct FadeAnimation {
    animation: Animation,
    from: f32,
    to: f32,
}

impl FadeAnimation {
    pub fn new(duration: Duration, from: f32, to: f32) -> Self {
        Self {
            animation: Animation::new(duration),
            from,
            to,
        }
    }

    pub fn fade_in(duration: Duration) -> Self {
        Self::new(duration, 0.0, 1.0)
    }

    pub fn fade_out(duration: Duration) -> Self {
        Self::new(duration, 1.0, 0.0)
    }

    pub fn value(&mut self) -> f32 {
        let t = self.animation.progress();
        let eased = Easing::ease_out_cubic(t);
        self.from + (self.to - self.from) * eased
    }

    pub fn is_complete(&self) -> bool {
        self.animation.is_complete()
    }
}

/// Slide animation helper
pub struct SlideAnimation {
    animation: Animation,
    from: f32,
    to: f32,
}

impl SlideAnimation {
    pub fn new(duration: Duration, from: f32, to: f32) -> Self {
        Self {
            animation: Animation::new(duration),
            from,
            to,
        }
    }

    pub fn value(&mut self) -> f32 {
        let t = self.animation.progress();
        let eased = Easing::ease_out_back(t);
        self.from + (self.to - self.from) * eased
    }

    pub fn is_complete(&self) -> bool {
        self.animation.is_complete()
    }
}

/// Scale animation helper
pub struct ScaleAnimation {
    animation: Animation,
    from: f32,
    to: f32,
}

impl ScaleAnimation {
    pub fn new(duration: Duration, from: f32, to: f32) -> Self {
        Self {
            animation: Animation::new(duration),
            from,
            to,
        }
    }

    pub fn value(&mut self) -> f32 {
        let t = self.animation.progress();
        let eased = Easing::ease_out_elastic(t);
        self.from + (self.to - self.from) * eased
    }

    pub fn is_complete(&self) -> bool {
        self.animation.is_complete()
    }
}

/// Spinner animation for loading states
pub struct SpinnerAnimation {
    start_time: Instant,
}

impl SpinnerAnimation {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
    }

    /// Get rotation angle in radians
    pub fn rotation(&self) -> f32 {
        let elapsed = self.start_time.elapsed().as_secs_f32();
        elapsed * std::f32::consts::TAU // Full rotation per second
    }
}

impl Default for SpinnerAnimation {
    fn default() -> Self {
        Self::new()
    }
}
