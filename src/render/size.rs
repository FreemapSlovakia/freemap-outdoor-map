use std::ops::Sub;

#[derive(PartialEq, Eq, Copy, Clone)]
pub struct Size<T>
where
    T: Sub<Output = T> + PartialOrd + Copy,
{
    pub width: T,
    pub height: T,
}

impl<T> Size<T>
where
    T: Sub<Output = T> + PartialOrd + Copy,
{
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}
