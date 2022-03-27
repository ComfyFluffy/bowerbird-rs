use std::net::TcpListener;

mod waitgroup;

pub use waitgroup::WaitGroup;

pub fn get_available_port<T>(ra: T) -> Option<u16>
where
    T: Iterator<Item = u16>,
{
    for p in ra {
        match TcpListener::bind(("127.0.0.1", p)) {
            Ok(_) => return Some(p),
            Err(e) => {
                eprintln!("{}", e)
            }
        }
    }
    None
}

pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let d = max - min;
    let mut h = if max == min {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    let s = if max == 0.0 { 0.0 } else { d / max };
    let v = max;
    (h, s, v)
}

#[cfg(test)]
mod tests {
    #[test]
    fn rgb2hsv() {
        assert_eq!(super::rgb_to_hsv(255, 0, 0), (0.0, 1.0, 1.0));
        assert_eq!(super::rgb_to_hsv(0, 255, 0), (120.0, 1.0, 1.0));
        assert_eq!(super::rgb_to_hsv(0, 0, 255), (240.0, 1.0, 1.0));
        assert_eq!(super::rgb_to_hsv(255, 255, 255), (0.0, 0.0, 1.0));
        assert_eq!(super::rgb_to_hsv(0, 0, 0), (0.0, 0.0, 0.0));
        // assert_eq!(super::rgb_to_hsv(108, 52, 62), (347.0, 0.52, 0.42));
    }
}

macro_rules! try_skip {
    ($res:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                warn!("{}", e);
                continue;
            }
        }
    };
}
pub(crate) use try_skip;
