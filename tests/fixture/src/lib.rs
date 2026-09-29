pub mod shapes {
    pub struct Circle {
        pub radius: f64,
    }

    impl Circle {
        pub fn new(radius: f64) -> Self {
            Self { radius }
        }

        pub fn area(&self) -> f64 {
            self.radius_squared() * std::f64::consts::PI
        }

        fn radius_squared(&self) -> f64 {
            helper::square(self.radius)
        }
    }

    mod helper {
        pub fn square(value: f64) -> f64 {
            value * value
        }
    }
}

pub mod style {
    pub trait Paint {
        fn paint(&self) -> u8 {
            7
        }
    }

    impl<T: Copy> Paint for T {}
}

pub mod prelude {
    pub use crate::style::Paint;
}

pub use shapes::Circle;

pub trait Pair {
    fn pair(self) -> u8;
}

impl Pair for (u8, u8) {
    fn pair(self) -> u8 {
        self.0
    }
}

impl Pair for &str {
    fn pair(self) -> u8 {
        1
    }
}

pub unsafe trait Raw {}

unsafe impl Raw for shapes::Circle {}

pub static mut COUNTER: u32 = 0;

pub struct Get;

#[cfg(feature = "gated")]
impl From<shapes::Circle> for Get {
    fn from(_: shapes::Circle) -> Self {
        Get
    }
}

pub fn get() -> u8 {
    5
}

#[cfg(feature = "gated")]
pub fn gated_only() -> u8 {
    1
}

#[cfg(feature = "extra")]
pub fn extra_default() -> u8 {
    2
}

pub fn always() -> u8 {
    3
}
