use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HueSettings {
    pub range: usize,
    pub colorize: bool,
    pub invert_range: bool,
    pub adjustments: [[f32; 3]; 7],
    pub bands: [[f32; 4]; 7],
}

impl Default for HueSettings {
    fn default() -> Self {
        Self {
            range: 0,
            colorize: false,
            invert_range: false,
            adjustments: [[0.0; 3]; 7],
            bands: [
                [0.0, 0.0, 360.0, 360.0],
                [315.0, 345.0, 15.0, 45.0],
                [15.0, 45.0, 75.0, 105.0],
                [75.0, 105.0, 135.0, 165.0],
                [135.0, 165.0, 195.0, 225.0],
                [195.0, 225.0, 255.0, 285.0],
                [255.0, 285.0, 315.0, 345.0],
            ],
        }
    }
}

impl HueSettings {
    pub const RANGES: [&'static str; 7] = [
        "Master", "Reds", "Yellows", "Greens", "Cyans", "Blues", "Magentas",
    ];

    pub fn response(&self, hue: f32) -> [f32; 3] {
        if self.colorize {
            return self.adjustments[self.range.min(6)];
        }
        let mut response = self.adjustments[0];
        for index in 1..7 {
            let band = self.bands[index];
            let span = (band[3] - band[0]).rem_euclid(360.0);
            let position = (hue - band[0]).rem_euclid(360.0);
            let ramp_in = (band[1] - band[0]).rem_euclid(360.0);
            let plateau_end = (band[2] - band[0]).rem_euclid(360.0);
            let mut weight = if span == 0.0 {
                1.0
            } else if position > span {
                0.0
            } else if position < ramp_in {
                position / ramp_in.max(0.0001)
            } else if position <= plateau_end {
                1.0
            } else {
                (span - position) / (span - plateau_end).max(0.0001)
            };
            if self.invert_range && self.range == index {
                weight = 1.0 - weight;
            }
            for (channel, value) in response.iter_mut().enumerate() {
                *value += self.adjustments[index][channel] * weight;
            }
        }
        response
    }

    pub fn valid(&self) -> bool {
        self.range < 7
            && self.adjustments.iter().all(|a| {
                a[0].is_finite()
                    && a[0].abs() <= 360.0
                    && a[1].is_finite()
                    && a[1].abs() <= 100.0
                    && a[2].is_finite()
                    && a[2].abs() <= 100.0
            })
            && self.bands.iter().flatten().all(|v| v.is_finite())
    }
}

pub const DEFAULT_LEVELS: [f32; 5] = [0.0, 1.0, 255.0, 0.0, 255.0];

pub fn level(value: f32, range: [f32; 5]) -> f32 {
    let [black, gamma, white, output_black, output_white] = range;
    let input = ((value * 255.0 - black) / (white - black).max(1.0)).clamp(0.0, 1.0);
    (output_black + input.powf(1.0 / gamma.max(0.01)) * (output_white - output_black)) / 255.0
}

pub fn decode_srgb(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub fn encode_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}
