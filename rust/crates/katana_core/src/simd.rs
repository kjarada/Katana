mod baseline;

#[cfg(all(
    feature = "simd-kernels",
    any(target_arch = "x86", target_arch = "x86_64")
))]
pub(crate) mod text_avx2;
#[cfg(all(feature = "simd-kernels", target_arch = "aarch64"))]
pub(crate) mod text_neon;

pub const BACKEND: &str = "portable";
pub const REGISTER_BYTES: usize = 16;

mod sealed {
    pub trait Sealed {}
}

pub trait Element: sealed::Sealed + Copy + Default + PartialOrd {
    type Storage: AsRef<[Self]> + AsMut<[Self]> + Copy + Default + std::fmt::Debug + PartialEq;
    type MaskStorage: AsRef<[bool]> + AsMut<[bool]> + Copy + Default + std::fmt::Debug + PartialEq;
    const LANES: usize;

    fn compare(left: &Self::Storage, right: &Self::Storage) -> Self::MaskStorage {
        let mut result = Self::MaskStorage::default();
        for ((mask, left), right) in result
            .as_mut()
            .iter_mut()
            .zip(left.as_ref())
            .zip(right.as_ref())
        {
            *mask = left < right;
        }
        result
    }

    fn select(
        mask: &Self::MaskStorage,
        when_true: &Self::Storage,
        when_false: &Self::Storage,
    ) -> Self::Storage {
        let mut result = Self::Storage::default();
        for (index, value) in result.as_mut().iter_mut().enumerate() {
            *value = if mask.as_ref()[index] {
                when_true.as_ref()[index]
            } else {
                when_false.as_ref()[index]
            };
        }
        result
    }
}

macro_rules! integer_elements {
    ($(($scalar:ty, $signed:expr)),+ $(,)?) => {$(
        impl sealed::Sealed for $scalar {}
        impl Element for $scalar {
            type Storage = [$scalar; REGISTER_BYTES / std::mem::size_of::<$scalar>()];
            type MaskStorage = [bool; REGISTER_BYTES / std::mem::size_of::<$scalar>()];
            const LANES: usize = REGISTER_BYTES / std::mem::size_of::<$scalar>();

            fn compare(left: &Self::Storage, right: &Self::Storage) -> Self::MaskStorage {
                baseline::compare_integer(left, right, $signed)
            }

            fn select(mask: &Self::MaskStorage, when_true: &Self::Storage, when_false: &Self::Storage) -> Self::Storage {
                baseline::select_integer(mask, when_true, when_false)
            }
        }
    )+};
}

integer_elements!(
    (i8, true),
    (u8, false),
    (i16, true),
    (u16, false),
    (i32, true),
    (u32, false),
    (i64, true),
    (u64, false),
    (isize, true),
    (usize, false)
);

impl sealed::Sealed for f64 {}
impl Element for f64 {
    type Storage = [f64; 2];
    type MaskStorage = [bool; 2];
    const LANES: usize = 2;

    fn compare(left: &Self::Storage, right: &Self::Storage) -> Self::MaskStorage {
        baseline::compare_f64(left, right)
    }

    fn select(
        mask: &Self::MaskStorage,
        when_true: &Self::Storage,
        when_false: &Self::Storage,
    ) -> Self::Storage {
        baseline::select_f64(mask, when_true, when_false)
    }
}

impl sealed::Sealed for f32 {}
impl Element for f32 {
    type Storage = [f32; 4];
    type MaskStorage = [bool; 4];
    const LANES: usize = 4;

    fn compare(left: &Self::Storage, right: &Self::Storage) -> Self::MaskStorage {
        baseline::compare_f32(left, right)
    }

    fn select(
        mask: &Self::MaskStorage,
        when_true: &Self::Storage,
        when_false: &Self::Storage,
    ) -> Self::Storage {
        baseline::select_f32(mask, when_true, when_false)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec<T: Element> {
    pub lane: T::Storage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mask<T: Element> {
    pub lane: T::MaskStorage,
}

pub use self::{Mask as PortableMask, Vec as PortableVec};

impl<T: Element> Vec<T> {
    pub const fn size() -> usize {
        T::LANES
    }

    pub fn less_than(&self, other: &Self) -> Mask<T> {
        Mask {
            lane: T::compare(&self.lane, &other.lane),
        }
    }
}

pub const fn lanes<T: Element>() -> usize {
    T::LANES
}

pub fn load<T: Element>(from: &[T]) -> Vec<T> {
    let mut value = Vec::<T>::default();
    value.lane.as_mut().copy_from_slice(&from[..T::LANES]);
    value
}

pub fn store<T: Element>(value: &Vec<T>, to: &mut [T]) {
    to[..T::LANES].copy_from_slice(value.lane.as_ref());
}

pub fn select<T: Element>(mask: &Mask<T>, when_true: &Vec<T>, when_false: &Vec<T>) -> Vec<T> {
    Vec {
        lane: T::select(&mask.lane, &when_true.lane, &when_false.lane),
    }
}

pub fn min_of<T: Element>(left: &Vec<T>, right: &Vec<T>) -> Vec<T> {
    select(&right.less_than(left), right, left)
}

pub fn max_of<T: Element>(left: &Vec<T>, right: &Vec<T>) -> Vec<T> {
    select(&left.less_than(right), right, left)
}
