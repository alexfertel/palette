//! Gamma encoding.

use core::marker::PhantomData;

use crate::encoding::TransferFn;
use crate::float::Float;
use crate::luma::LumaStandard;
use crate::rgb::{RgbSpace, RgbStandard};
use crate::white_point::WhitePoint;
use crate::{from_f64, FromF64};

/// Gamma encoding.
///
/// Gamma encoding or gamma correction is used to transform the intensity
/// values to either match a non-linear display, like CRT, or to prevent
/// banding among the darker colors. `GammaRgb` represents a gamma corrected
/// RGB color, where the intensities are encoded using the following power-law
/// expression: _V<sup> γ</sup>_ (where _V_ is the intensity value an _γ_ is the
/// encoding gamma).
///
/// The gamma value is stored as a simple type that represents an `f32`
/// constant.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Gamma<S, N: Number = F2p2>(PhantomData<(S, N)>);

impl<T, Sp, N> RgbStandard<T> for Gamma<Sp, N>
where
    T: Float + FromF64,
    Sp: RgbSpace<T>,
    N: Number,
{
    type Space = Sp;
    type TransferFn = GammaFn<N>;
}

impl<T, Wp, N> LumaStandard<T> for Gamma<Wp, N>
where
    T: Float + FromF64,
    Wp: WhitePoint<T>,
    N: Number,
{
    type WhitePoint = Wp;
    type TransferFn = GammaFn<N>;
}

/// The transfer function for gamma encoded colors.
///
/// The gamma value is stored as a simple type that represents an `f32`
/// constant.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GammaFn<N: Number = F2p2>(PhantomData<N>);

impl<T, N> TransferFn<T> for GammaFn<N>
where
    T: Float + FromF64,
    N: Number,
{
    #[inline]
    fn into_linear(x: T) -> T {
        x.powf(T::one() / from_f64(N::VALUE))
    }

    #[inline]
    fn from_linear(x: T) -> T {
        x.powf(from_f64(N::VALUE))
    }
}

/// A type level float constant.
pub trait Number: 'static {
    /// The represented number.
    const VALUE: f64;
}

/// Represents `2.2f64`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct F2p2;

impl Number for F2p2 {
    const VALUE: f64 = 2.2;
}
