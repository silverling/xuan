use crate::document::Point;

/// Projective mapping from the unit square to a convex quadrilateral.
#[derive(Clone, Copy, Debug)]
pub struct Homography(pub [[f32; 3]; 3]);

impl Homography {
    pub fn from_quad(p: [Point; 4]) -> Option<Self> {
        if !p.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
            return None;
        }
        let mut sign = 0.0_f32;
        for i in 0..4 {
            let a = p[i];
            let b = p[(i + 1) % 4];
            let c = p[(i + 2) % 4];
            let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
            if cross.abs() < 1e-7 || (sign != 0.0 && cross.signum() != sign) {
                return None;
            }
            sign = cross.signum();
        }
        let dx1 = p[1].x - p[2].x;
        let dx2 = p[3].x - p[2].x;
        let dy1 = p[1].y - p[2].y;
        let dy2 = p[3].y - p[2].y;
        let dx3 = p[0].x - p[1].x + p[2].x - p[3].x;
        let dy3 = p[0].y - p[1].y + p[2].y - p[3].y;
        let determinant = dx1 * dy2 - dx2 * dy1;
        if determinant.abs() < 1e-8 {
            return None;
        }
        let g = (dx3 * dy2 - dx2 * dy3) / determinant;
        let h = (dx1 * dy3 - dx3 * dy1) / determinant;
        Some(Self([
            [
                p[1].x - p[0].x + g * p[1].x,
                p[3].x - p[0].x + h * p[3].x,
                p[0].x,
            ],
            [
                p[1].y - p[0].y + g * p[1].y,
                p[3].y - p[0].y + h * p[3].y,
                p[0].y,
            ],
            [g, h, 1.0],
        ]))
    }

    pub fn map(self, p: Point) -> Point {
        let m = self.0;
        let divisor = m[2][0] * p.x + m[2][1] * p.y + m[2][2];
        Point::new(
            (m[0][0] * p.x + m[0][1] * p.y + m[0][2]) / divisor,
            (m[1][0] * p.x + m[1][1] * p.y + m[1][2]) / divisor,
        )
    }

    pub fn inverse(self) -> Option<Self> {
        let [[a, b, c], [d, e, f], [g, h, i]] = self.0;
        let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
        if determinant.abs() < 1e-8 {
            return None;
        }
        Some(Self(
            [
                [e * i - f * h, c * h - b * i, b * f - c * e],
                [f * g - d * i, a * i - c * g, c * d - a * f],
                [d * h - e * g, b * g - a * h, a * e - b * d],
            ]
            .map(|row| row.map(|v| v / determinant)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn perspective_round_trip_and_fold_rejection() {
        let quad = [
            Point::new(0.2, 0.1),
            Point::new(0.9, 0.0),
            Point::new(1.2, 1.0),
            Point::new(-0.1, 1.1),
        ];
        let h = Homography::from_quad(quad).unwrap();
        let inverse = h.inverse().unwrap();
        for point in [
            Point::new(0.0, 0.0),
            Point::new(0.3, 0.6),
            Point::new(1.0, 1.0),
        ] {
            assert!(inverse.map(h.map(point)).distance(point) < 0.00001);
        }
        assert!(Homography::from_quad([quad[0], quad[2], quad[1], quad[3]]).is_none());
    }
}
