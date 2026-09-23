use xuan::document::Point;

/// A spatial low-pass filter. Its length is measured in screen points so zoom
/// and the number of input events do not change the feel of the brush.
pub(super) struct StrokeSmoother {
    pub input: Point,
    output: Point,
    length: f32,
}

impl StrokeSmoother {
    pub fn new(start: Point, strength: f32, zoom: f32) -> Self {
        Self {
            input: start,
            output: start,
            length: 32.0 * strength.clamp(0.0, 1.0) / zoom.max(0.01),
        }
    }

    pub fn update(&mut self, point: Point) -> Point {
        let distance = self.input.distance(point);
        if self.length <= 0.0 {
            self.output = point;
        } else if distance > 0.0 {
            // Integrate the filter along the entire input segment, rather than
            // weighting each event equally. Splitting a straight movement into
            // more events produces the same result. f64 avoids cancellation for
            // very short segments relative to the smoothing length.
            let ratio = f64::from(distance) / f64::from(self.length);
            let follow = -(-ratio).exp_m1();
            let advance = (1.0 - follow / ratio) as f32;
            let follow = follow as f32;
            self.output = Point::new(
                self.output.x
                    + (self.input.x - self.output.x) * follow
                    + (point.x - self.input.x) * advance,
                self.output.y
                    + (self.input.y - self.output.y) * follow
                    + (point.y - self.input.y) * advance,
            );
        }
        self.input = point;
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_reduces_jitter_and_increases_with_strength() {
        let mut weak = StrokeSmoother::new(Point::default(), 0.25, 1.0);
        let mut strong = StrokeSmoother::new(Point::default(), 1.0, 1.0);
        let mut weak_motion = 0.0;
        let mut strong_motion = 0.0;
        for i in 1..100 {
            let point = Point::new(i as f32 * 2.0, if i % 2 == 0 { 3.0 } else { -3.0 });
            weak_motion += weak.update(point).y.abs();
            strong_motion += strong.update(point).y.abs();
        }
        assert!(weak_motion < 99.0 * 3.0 * 0.5);
        assert!(strong_motion < weak_motion);
        assert!(strong.output.x < weak.output.x);
    }

    #[test]
    fn smoothing_is_independent_of_collinear_event_density_and_zoom() {
        let mut sparse = StrokeSmoother::new(Point::default(), 0.75, 1.0);
        let mut dense = StrokeSmoother::new(Point::default(), 0.75, 1.0);
        let mut zoomed = StrokeSmoother::new(Point::default(), 0.75, 4.0);
        for end in [Point::new(100.0, 50.0), Point::new(20.0, 150.0)] {
            let start = dense.input;
            for step in 1..=100 {
                let t = step as f32 / 100.0;
                dense.update(Point::new(
                    start.x + (end.x - start.x) * t,
                    start.y + (end.y - start.y) * t,
                ));
            }
            let expected = sparse.update(end);
            assert!(dense.output.distance(expected) < 0.001);
            let actual = zoomed.update(Point::new(end.x / 4.0, end.y / 4.0));
            assert!(Point::new(actual.x * 4.0, actual.y * 4.0).distance(expected) < 0.001);
        }
    }

    #[test]
    fn smoothing_preserves_taps_stationary_input_and_disabled_paths() {
        let start = Point::new(10.0, 20.0);
        let mut smoother = StrokeSmoother::new(start, 1.0, 1.0);
        assert_eq!(smoother.update(start), start);
        let end = Point::new(30.0, 40.0);
        let output = smoother.update(end);
        for _ in 0..100 {
            assert_eq!(smoother.update(end), output);
        }
        let mut disabled = StrokeSmoother::new(start, 0.0, 1.0);
        assert_eq!(disabled.update(end), end);
        assert_eq!(disabled.update(start), start);
    }
}
