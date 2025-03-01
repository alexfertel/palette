use core::any::TypeId;
use core::fmt;
use core::marker::PhantomData;
use core::num::ParseIntError;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};
use core::str::FromStr;

use approx::{AbsDiffEq, RelativeEq, UlpsEq};
use num_traits::Zero;
#[cfg(feature = "random")]
use rand::distributions::uniform::{SampleBorrow, SampleUniform, Uniform, UniformSampler};
#[cfg(feature = "random")]
use rand::distributions::{Distribution, Standard};
#[cfg(feature = "random")]
use rand::Rng;

use crate::alpha::Alpha;
use crate::blend::PreAlpha;
use crate::cast::{ComponentOrder, Packed};
use crate::convert::FromColorUnclamped;
use crate::encoding::linear::LinearFn;
use crate::encoding::{Linear, Srgb};
use crate::luma::LumaStandard;
use crate::matrix::{matrix_inverse, multiply_xyz_to_rgb, rgb_to_xyz_matrix};
use crate::rgb::{RgbSpace, RgbStandard, TransferFn};
use crate::{
    clamp, clamp_assign, clamp_min_assign, contrast_ratio, from_f64, Blend, Clamp, ClampAssign,
    Component, ComponentWise, FloatComponent, FromComponent, GetHue, IsWithinBounds, Lighten,
    LightenAssign, Mix, MixAssign, RelativeContrast,
};
use crate::{Hsl, Hsv, Luma, RgbHue, Xyz};

/// Generic RGB with an alpha component. See the [`Rgba` implementation in
/// `Alpha`](crate::Alpha#Rgba).
pub type Rgba<S = Srgb, T = f32> = Alpha<Rgb<S, T>, T>;

/// Generic RGB.
///
/// RGB is probably the most common color space, when it comes to computer
/// graphics, and it's defined as an additive mixture of red, green and blue
/// light, where gray scale colors are created when these three channels are
/// equal in strength.
///
/// Many conversions and operations on this color space requires that it's
/// linear, meaning that gamma correction is required when converting to and
/// from a displayable RGB, such as sRGB. See the [`encoding`](crate::encoding)
/// module for encoding formats.
#[derive(Debug, ArrayCast, FromColorUnclamped, WithAlpha)]
#[cfg_attr(feature = "serializing", derive(Serialize, Deserialize))]
#[palette(
    palette_internal,
    rgb_standard = "S",
    component = "T",
    skip_derives(Xyz, Hsv, Hsl, Luma, Rgb)
)]
#[repr(C)]
pub struct Rgb<S = Srgb, T = f32> {
    /// The amount of red light, where 0.0 is no red light and 1.0f (or 255u8)
    /// is the highest displayable amount.
    pub red: T,

    /// The amount of green light, where 0.0 is no green light and 1.0f (or
    /// 255u8) is the highest displayable amount.
    pub green: T,

    /// The amount of blue light, where 0.0 is no blue light and 1.0f (or
    /// 255u8) is the highest displayable amount.
    pub blue: T,

    /// The kind of RGB standard. sRGB is the default.
    #[cfg_attr(feature = "serializing", serde(skip))]
    #[palette(unsafe_zero_sized)]
    pub standard: PhantomData<S>,
}

impl<S, T: Copy> Copy for Rgb<S, T> {}

impl<S, T: Clone> Clone for Rgb<S, T> {
    fn clone(&self) -> Rgb<S, T> {
        Rgb {
            red: self.red.clone(),
            green: self.green.clone(),
            blue: self.blue.clone(),
            standard: PhantomData,
        }
    }
}

impl<S, T> Rgb<S, T> {
    /// Create an RGB color.
    pub const fn new(red: T, green: T, blue: T) -> Rgb<S, T> {
        Rgb {
            red,
            green,
            blue,
            standard: PhantomData,
        }
    }

    /// Convert into another component type.
    pub fn into_format<U>(self) -> Rgb<S, U>
    where
        T: Component,
        U: FromComponent<T>,
    {
        Rgb {
            red: U::from_component(self.red),
            green: U::from_component(self.green),
            blue: U::from_component(self.blue),
            standard: PhantomData,
        }
    }

    /// Convert from another component type.
    pub fn from_format<U>(color: Rgb<S, U>) -> Self
    where
        T: FromComponent<U>,
        U: Component,
    {
        color.into_format()
    }

    /// Convert to a `(red, green, blue)` tuple.
    pub fn into_components(self) -> (T, T, T) {
        (self.red, self.green, self.blue)
    }

    /// Convert from a `(red, green, blue)` tuple.
    pub fn from_components((red, green, blue): (T, T, T)) -> Self {
        Self::new(red, green, blue)
    }
}

impl<S, T> Rgb<S, T>
where
    T: Component,
{
    /// Return the `red` value minimum.
    pub fn min_red() -> T {
        T::zero()
    }

    /// Return the `red` value maximum.
    pub fn max_red() -> T {
        T::max_intensity()
    }

    /// Return the `green` value minimum.
    pub fn min_green() -> T {
        T::zero()
    }

    /// Return the `green` value maximum.
    pub fn max_green() -> T {
        T::max_intensity()
    }

    /// Return the `blue` value minimum.
    pub fn min_blue() -> T {
        T::zero()
    }

    /// Return the `blue` value maximum.
    pub fn max_blue() -> T {
        T::max_intensity()
    }
}

impl<S, T> PartialEq for Rgb<S, T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.red == other.red && self.green == other.green && self.blue == other.blue
    }
}

impl<S, T> Eq for Rgb<S, T> where T: Eq {}

impl<S> Rgb<S, u8> {
    /// Convert to a packed `u32` with with specifiable component order.
    ///
    /// ```
    /// use palette::{rgb, Srgb};
    ///
    /// let integer = Srgb::new(96u8, 127, 0).into_u32::<rgb::channels::Rgba>();
    /// assert_eq!(0x607F00FF, integer);
    /// ```
    ///
    /// It's also possible to use `From` and `Into`, which defaults to the
    /// `0xAARRGGBB` component order:
    ///
    /// ```
    /// use palette::Srgb;
    ///
    /// let integer = u32::from(Srgb::new(96u8, 127, 0));
    /// assert_eq!(0xFF607F00, integer);
    /// ```
    ///
    /// See [Packed](crate::cast::Packed) for more details.
    #[inline]
    pub fn into_u32<O>(self) -> u32
    where
        O: ComponentOrder<Rgba<S, u8>, u32>,
    {
        O::pack(Rgba::from(self))
    }

    /// Convert from a packed `u32` with specifiable component order.
    ///
    /// ```
    /// use palette::{rgb, Srgb};
    ///
    /// let rgb = Srgb::from_u32::<rgb::channels::Rgba>(0x607F00FF);
    /// assert_eq!(Srgb::new(96u8, 127, 0), rgb);
    /// ```
    ///
    /// It's also possible to use `From` and `Into`, which defaults to the
    /// `0xAARRGGBB` component order:
    ///
    /// ```
    /// use palette::Srgb;
    ///
    /// let rgb = Srgb::from(0x607F00);
    /// assert_eq!(Srgb::new(96u8, 127, 0), rgb);
    /// ```
    ///
    /// See [Packed](crate::cast::Packed) for more details.
    #[inline]
    pub fn from_u32<O>(color: u32) -> Self
    where
        O: ComponentOrder<Rgba<S, u8>, u32>,
    {
        O::unpack(color).color
    }
}

impl<S: RgbStandard<T>, T: FloatComponent> Rgb<S, T> {
    /// Convert the color to linear RGB.
    pub fn into_linear(self) -> Rgb<Linear<S::Space>, T> {
        Rgb::new(
            S::TransferFn::into_linear(self.red),
            S::TransferFn::into_linear(self.green),
            S::TransferFn::into_linear(self.blue),
        )
    }

    /// Convert linear RGB to non-linear RGB.
    pub fn from_linear(color: Rgb<Linear<S::Space>, T>) -> Rgb<S, T> {
        Rgb::new(
            S::TransferFn::from_linear(color.red),
            S::TransferFn::from_linear(color.green),
            S::TransferFn::from_linear(color.blue),
        )
    }

    /// Convert the color to a different encoding.
    pub fn into_encoding<St>(self) -> Rgb<St, T>
    where
        St: RgbStandard<T, Space = S::Space>,
    {
        Rgb::new(
            St::TransferFn::from_linear(S::TransferFn::into_linear(self.red)),
            St::TransferFn::from_linear(S::TransferFn::into_linear(self.green)),
            St::TransferFn::from_linear(S::TransferFn::into_linear(self.blue)),
        )
    }

    /// Convert RGB from a different encoding.
    pub fn from_encoding<St>(color: Rgb<St, T>) -> Rgb<S, T>
    where
        St: RgbStandard<T, Space = S::Space>,
    {
        Rgb::new(
            S::TransferFn::from_linear(St::TransferFn::into_linear(color.red)),
            S::TransferFn::from_linear(St::TransferFn::into_linear(color.green)),
            S::TransferFn::from_linear(St::TransferFn::into_linear(color.blue)),
        )
    }
}

impl<S, T> Rgb<S, T>
where
    S: RgbStandard<T>,
{
    #[inline]
    fn reinterpret_as<St>(self) -> Rgb<St, T>
    where
        S::Space: RgbSpace<T, WhitePoint = <St::Space as RgbSpace<T>>::WhitePoint>,
        St: RgbStandard<T>,
    {
        Rgb {
            red: self.red,
            green: self.green,
            blue: self.blue,
            standard: PhantomData,
        }
    }
}

/// <span id="Rgba"></span>[`Rgba`](crate::rgb::Rgba) implementations.
impl<S, T, A> Alpha<Rgb<S, T>, A> {
    /// Non-linear RGB.
    pub const fn new(red: T, green: T, blue: T, alpha: A) -> Self {
        Alpha {
            color: Rgb::new(red, green, blue),
            alpha,
        }
    }

    /// Convert into another component type.
    pub fn into_format<U, B>(self) -> Alpha<Rgb<S, U>, B>
    where
        T: Component,
        A: Component,
        U: FromComponent<T>,
        B: FromComponent<A>,
    {
        Alpha::<Rgb<S, U>, B>::new(
            U::from_component(self.red),
            U::from_component(self.green),
            U::from_component(self.blue),
            B::from_component(self.alpha),
        )
    }

    /// Convert from another component type.
    pub fn from_format<U, B>(color: Alpha<Rgb<S, U>, B>) -> Self
    where
        T: FromComponent<U>,
        U: Component,
        A: FromComponent<B>,
        B: Component,
    {
        color.into_format()
    }

    /// Convert to a `(red, green, blue, alpha)` tuple.
    pub fn into_components(self) -> (T, T, T, A) {
        (
            self.color.red,
            self.color.green,
            self.color.blue,
            self.alpha,
        )
    }

    /// Convert from a `(red, green, blue, alpha)` tuple.
    pub fn from_components((red, green, blue, alpha): (T, T, T, A)) -> Self {
        Self::new(red, green, blue, alpha)
    }
}

impl<S> Rgba<S, u8> {
    /// Convert to a packed `u32` with with specifiable component order.
    ///
    /// ```
    /// use palette::{rgb, Srgba};
    ///
    /// let integer = Srgba::new(96u8, 127, 0, 255).into_u32::<rgb::channels::Argb>();
    /// assert_eq!(0xFF607F00, integer);
    /// ```
    ///
    /// It's also possible to use `From` and `Into`, which defaults to the
    /// `0xRRGGBBAA` component order:
    ///
    /// ```
    /// use palette::Srgba;
    ///
    /// let integer = u32::from(Srgba::new(96u8, 127, 0, 255));
    /// assert_eq!(0x607F00FF, integer);
    /// ```
    ///
    /// See [Packed](crate::cast::Packed) for more details.
    #[inline]
    pub fn into_u32<O>(self) -> u32
    where
        O: ComponentOrder<Rgba<S, u8>, u32>,
    {
        O::pack(self)
    }

    /// Convert from a packed `u32` with specifiable component order.
    ///
    /// ```
    /// use palette::{rgb, Srgba};
    ///
    /// let rgba = Srgba::from_u32::<rgb::channels::Argb>(0xFF607F00);
    /// assert_eq!(Srgba::new(96u8, 127, 0, 255), rgba);
    /// ```
    ///
    /// It's also possible to use `From` and `Into`, which defaults to the
    /// `0xRRGGBBAA` component order:
    ///
    /// ```
    /// use palette::Srgba;
    ///
    /// let rgba = Srgba::from(0x607F00FF);
    /// assert_eq!(Srgba::new(96u8, 127, 0, 255), rgba);
    /// ```
    ///
    /// See [Packed](crate::cast::Packed) for more details.
    #[inline]
    pub fn from_u32<O>(color: u32) -> Self
    where
        O: ComponentOrder<Rgba<S, u8>, u32>,
    {
        O::unpack(color)
    }
}

/// [`Rgba`](crate::rgb::Rgba) implementations.
impl<S: RgbStandard<T>, T: FloatComponent, A> Alpha<Rgb<S, T>, A> {
    /// Convert the color to linear RGB with transparency.
    pub fn into_linear(self) -> Alpha<Rgb<Linear<S::Space>, T>, A> {
        Alpha::<Rgb<Linear<S::Space>, T>, A>::new(
            S::TransferFn::into_linear(self.red),
            S::TransferFn::into_linear(self.green),
            S::TransferFn::into_linear(self.blue),
            self.alpha,
        )
    }

    /// Convert linear RGB to non-linear RGB with transparency.
    pub fn from_linear(color: Alpha<Rgb<Linear<S::Space>, T>, A>) -> Self {
        Self::new(
            S::TransferFn::from_linear(color.red),
            S::TransferFn::from_linear(color.green),
            S::TransferFn::from_linear(color.blue),
            color.alpha,
        )
    }

    /// Convert the color to a different encoding with transparency.
    pub fn into_encoding<St>(self) -> Alpha<Rgb<St, T>, A>
    where
        St: RgbStandard<T, Space = S::Space>,
    {
        Alpha::<Rgb<St, T>, A>::new(
            St::TransferFn::from_linear(S::TransferFn::into_linear(self.red)),
            St::TransferFn::from_linear(S::TransferFn::into_linear(self.green)),
            St::TransferFn::from_linear(S::TransferFn::into_linear(self.blue)),
            self.alpha,
        )
    }

    /// Convert RGB from a different encoding with transparency.
    pub fn from_encoding<St>(color: Alpha<Rgb<St, T>, A>) -> Self
    where
        St: RgbStandard<T, Space = S::Space>,
    {
        Self::new(
            S::TransferFn::from_linear(St::TransferFn::into_linear(color.red)),
            S::TransferFn::from_linear(St::TransferFn::into_linear(color.green)),
            S::TransferFn::from_linear(St::TransferFn::into_linear(color.blue)),
            color.alpha,
        )
    }
}

impl<S1, S2, T> FromColorUnclamped<Rgb<S2, T>> for Rgb<S1, T>
where
    S1: RgbStandard<T>,
    S2: RgbStandard<T>,
    S2::Space: RgbSpace<T, WhitePoint = <S1::Space as RgbSpace<T>>::WhitePoint>,
    T: FloatComponent,
{
    fn from_color_unclamped(rgb: Rgb<S2, T>) -> Self {
        let rgb_space1 = TypeId::of::<<S1::Space as RgbSpace<T>>::Primaries>();
        let rgb_space2 = TypeId::of::<<S2::Space as RgbSpace<T>>::Primaries>();

        if TypeId::of::<S1>() == TypeId::of::<S2>() {
            rgb.reinterpret_as()
        } else if rgb_space1 == rgb_space2 {
            Self::from_linear(rgb.into_linear().reinterpret_as())
        } else {
            Self::from_color_unclamped(Xyz::from_color_unclamped(rgb))
        }
    }
}

impl<S, T> FromColorUnclamped<Xyz<<S::Space as RgbSpace<T>>::WhitePoint, T>> for Rgb<S, T>
where
    S: RgbStandard<T>,
    T: FloatComponent,
{
    fn from_color_unclamped(color: Xyz<<S::Space as RgbSpace<T>>::WhitePoint, T>) -> Self {
        let transform_matrix = matrix_inverse(&rgb_to_xyz_matrix::<S::Space, T>());
        Self::from_linear(multiply_xyz_to_rgb(&transform_matrix, &color))
    }
}

impl<S, T> FromColorUnclamped<Hsl<S, T>> for Rgb<S, T>
where
    T: FloatComponent,
{
    fn from_color_unclamped(hsl: Hsl<S, T>) -> Self {
        let c = (T::one() - (hsl.lightness * from_f64(2.0) - T::one()).abs()) * hsl.saturation;
        let h = hsl.hue.to_positive_degrees() / from_f64(60.0);
        let x = c * (T::one() - (h % from_f64(2.0) - T::one()).abs());
        let m = hsl.lightness - c * from_f64(0.5);

        let (red, green, blue) = if h >= T::zero() && h < T::one() {
            (c, x, T::zero())
        } else if h >= T::one() && h < from_f64(2.0) {
            (x, c, T::zero())
        } else if h >= from_f64(2.0) && h < from_f64(3.0) {
            (T::zero(), c, x)
        } else if h >= from_f64(3.0) && h < from_f64(4.0) {
            (T::zero(), x, c)
        } else if h >= from_f64(4.0) && h < from_f64(5.0) {
            (x, T::zero(), c)
        } else {
            (c, T::zero(), x)
        };

        Rgb {
            red: red + m,
            green: green + m,
            blue: blue + m,
            standard: PhantomData,
        }
    }
}

impl<S, T> FromColorUnclamped<Hsv<S, T>> for Rgb<S, T>
where
    T: FloatComponent,
{
    fn from_color_unclamped(hsv: Hsv<S, T>) -> Self {
        let c = hsv.value * hsv.saturation;
        let h = hsv.hue.to_positive_degrees() / from_f64(60.0);
        let x = c * (T::one() - (h % from_f64(2.0) - T::one()).abs());
        let m = hsv.value - c;

        let (red, green, blue) = if h >= T::zero() && h < T::one() {
            (c, x, T::zero())
        } else if h >= T::one() && h < from_f64(2.0) {
            (x, c, T::zero())
        } else if h >= from_f64(2.0) && h < from_f64(3.0) {
            (T::zero(), c, x)
        } else if h >= from_f64(3.0) && h < from_f64(4.0) {
            (T::zero(), x, c)
        } else if h >= from_f64(4.0) && h < from_f64(5.0) {
            (x, T::zero(), c)
        } else {
            (c, T::zero(), x)
        };

        Rgb {
            red: red + m,
            green: green + m,
            blue: blue + m,
            standard: PhantomData,
        }
    }
}

impl<S, St, T> FromColorUnclamped<Luma<St, T>> for Rgb<S, T>
where
    S: RgbStandard<T>,
    St: LumaStandard<T, WhitePoint = <S::Space as RgbSpace<T>>::WhitePoint>,
    T: FloatComponent,
{
    fn from_color_unclamped(color: Luma<St, T>) -> Self {
        let luma = color.into_linear();

        Self::from_linear(Rgb {
            red: luma.luma,
            green: luma.luma,
            blue: luma.luma,
            standard: PhantomData,
        })
    }
}

impl<S, T> IsWithinBounds for Rgb<S, T>
where
    T: Component,
{
    #[rustfmt::skip]
    #[inline]
    fn is_within_bounds(&self) -> bool {
        self.red >= Self::min_red() && self.red <= Self::max_red() &&
        self.green >= Self::min_green() && self.green <= Self::max_green() &&
        self.blue >= Self::min_blue() && self.blue <= Self::max_blue()
    }
}

impl<S, T> Clamp for Rgb<S, T>
where
    T: Component,
{
    #[inline]
    fn clamp(self) -> Self {
        Self::new(
            clamp(self.red, Self::min_red(), Self::max_red()),
            clamp(self.green, Self::min_green(), Self::max_green()),
            clamp(self.blue, Self::min_blue(), Self::max_blue()),
        )
    }
}

impl<S, T> ClampAssign for Rgb<S, T>
where
    T: Component,
{
    #[inline]
    fn clamp_assign(&mut self) {
        clamp_assign(&mut self.red, Self::min_red(), Self::max_red());
        clamp_assign(&mut self.green, Self::min_green(), Self::max_green());
        clamp_assign(&mut self.blue, Self::min_blue(), Self::max_blue());
    }
}

impl<S, T> Mix for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: FloatComponent,
{
    type Scalar = T;

    #[inline]
    fn mix(self, other: Self, factor: T) -> Self {
        let factor = clamp(factor, T::zero(), T::one());
        self + (other - self) * factor
    }
}

impl<S, T> MixAssign for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: FloatComponent + AddAssign,
{
    type Scalar = T;

    #[inline]
    fn mix_assign(&mut self, other: Self, factor: T) {
        let factor = clamp(factor, T::zero(), T::one());
        *self += (other - *self) * factor;
    }
}

impl<S, T> Lighten for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: FloatComponent,
{
    type Scalar = T;

    #[inline]
    fn lighten(self, factor: T) -> Self {
        let difference_red = if factor >= T::zero() {
            Self::max_red() - self.red
        } else {
            self.red
        };
        let delta_red = difference_red.max(T::zero()) * factor;

        let difference_green = if factor >= T::zero() {
            Self::max_green() - self.green
        } else {
            self.green
        };
        let delta_green = difference_green.max(T::zero()) * factor;

        let difference_blue = if factor >= T::zero() {
            Self::max_blue() - self.blue
        } else {
            self.blue
        };
        let delta_blue = difference_blue.max(T::zero()) * factor;

        Rgb {
            red: (self.red + delta_red).max(Self::min_red()),
            green: (self.green + delta_green).max(Self::min_green()),
            blue: (self.blue + delta_blue).max(Self::min_blue()),
            standard: PhantomData,
        }
    }

    #[inline]
    fn lighten_fixed(self, amount: T) -> Self {
        Rgb {
            red: (self.red + Self::max_red() * amount).max(Self::min_red()),
            green: (self.green + Self::max_green() * amount).max(Self::min_green()),
            blue: (self.blue + Self::max_blue() * amount).max(Self::min_blue()),
            standard: PhantomData,
        }
    }
}

impl<S, T> LightenAssign for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: FloatComponent + AddAssign,
{
    type Scalar = T;

    #[inline]
    fn lighten_assign(&mut self, factor: T) {
        let difference_red = if factor >= T::zero() {
            Self::max_red() - self.red
        } else {
            self.red
        };
        self.red += difference_red.max(T::zero()) * factor;
        clamp_min_assign(&mut self.red, Self::min_red());

        let difference_green = if factor >= T::zero() {
            Self::max_green() - self.green
        } else {
            self.green
        };
        self.green += difference_green.max(T::zero()) * factor;
        clamp_min_assign(&mut self.green, Self::min_green());

        let difference_blue = if factor >= T::zero() {
            Self::max_blue() - self.blue
        } else {
            self.blue
        };
        self.blue += difference_blue.max(T::zero()) * factor;
        clamp_min_assign(&mut self.blue, Self::min_blue());
    }

    #[inline]
    fn lighten_fixed_assign(&mut self, amount: T) {
        self.red += Self::max_red() * amount;
        clamp_min_assign(&mut self.red, Self::min_red());
        self.green += Self::max_green() * amount;
        clamp_min_assign(&mut self.green, Self::min_green());
        self.blue += Self::max_blue() * amount;
        clamp_min_assign(&mut self.blue, Self::min_blue());
    }
}

impl<S, T> GetHue for Rgb<S, T>
where
    T: FloatComponent,
{
    type Hue = RgbHue<T>;

    fn get_hue(&self) -> Option<RgbHue<T>> {
        let sqrt_3: T = from_f64(1.73205081);

        if self.red == self.green && self.red == self.blue {
            None
        } else {
            Some(RgbHue::from_radians(
                (sqrt_3 * (self.green - self.blue))
                    .atan2(self.red * from_f64(2.0) - self.green - self.blue),
            ))
        }
    }
}

impl<S, T> Blend for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: FloatComponent,
{
    type Color = Rgb<S, T>;

    fn into_premultiplied(self) -> PreAlpha<Rgb<S, T>, T> {
        Rgba {
            color: self,
            alpha: T::one(),
        }
        .into_premultiplied()
    }

    fn from_premultiplied(color: PreAlpha<Rgb<S, T>, T>) -> Self {
        Rgba::from_premultiplied(color).color
    }
}

impl<S, T> ComponentWise for Rgb<S, T>
where
    T: Clone,
{
    type Scalar = T;

    fn component_wise<F: FnMut(T, T) -> T>(&self, other: &Rgb<S, T>, mut f: F) -> Rgb<S, T> {
        Rgb {
            red: f(self.red.clone(), other.red.clone()),
            green: f(self.green.clone(), other.green.clone()),
            blue: f(self.blue.clone(), other.blue.clone()),
            standard: PhantomData,
        }
    }

    fn component_wise_self<F: FnMut(T) -> T>(&self, mut f: F) -> Rgb<S, T> {
        Rgb {
            red: f(self.red.clone()),
            green: f(self.green.clone()),
            blue: f(self.blue.clone()),
            standard: PhantomData,
        }
    }
}

impl<S, T> Default for Rgb<S, T>
where
    T: Zero,
{
    fn default() -> Rgb<S, T> {
        Rgb::new(T::zero(), T::zero(), T::zero())
    }
}

impl<S, T> Add<Rgb<S, T>> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: Add,
{
    type Output = Rgb<S, <T as Add>::Output>;

    fn add(self, other: Rgb<S, T>) -> Self::Output {
        Rgb {
            red: self.red + other.red,
            green: self.green + other.green,
            blue: self.blue + other.blue,
            standard: PhantomData,
        }
    }
}

impl<S, T> Add<T> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: Add + Clone,
{
    type Output = Rgb<S, <T as Add>::Output>;

    fn add(self, c: T) -> Self::Output {
        Rgb {
            red: self.red + c.clone(),
            green: self.green + c.clone(),
            blue: self.blue + c,
            standard: PhantomData,
        }
    }
}

impl<S, T> AddAssign<Rgb<S, T>> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: AddAssign,
{
    fn add_assign(&mut self, other: Rgb<S, T>) {
        self.red += other.red;
        self.green += other.green;
        self.blue += other.blue;
    }
}

impl<S, T> AddAssign<T> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: AddAssign + Clone,
{
    fn add_assign(&mut self, c: T) {
        self.red += c.clone();
        self.green += c.clone();
        self.blue += c;
    }
}

impl<S, T> Sub<Rgb<S, T>> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: Sub,
{
    type Output = Rgb<S, <T as Sub>::Output>;

    fn sub(self, other: Rgb<S, T>) -> Self::Output {
        Rgb {
            red: self.red - other.red,
            green: self.green - other.green,
            blue: self.blue - other.blue,
            standard: PhantomData,
        }
    }
}

impl<S, T> Sub<T> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: Sub + Clone,
{
    type Output = Rgb<S, <T as Sub>::Output>;

    fn sub(self, c: T) -> Self::Output {
        Rgb {
            red: self.red - c.clone(),
            green: self.green - c.clone(),
            blue: self.blue - c,
            standard: PhantomData,
        }
    }
}

impl<S, T> SubAssign<Rgb<S, T>> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: SubAssign,
{
    fn sub_assign(&mut self, other: Rgb<S, T>) {
        self.red -= other.red;
        self.green -= other.green;
        self.blue -= other.blue;
    }
}

impl<S, T> SubAssign<T> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: SubAssign + Clone,
{
    fn sub_assign(&mut self, c: T) {
        self.red -= c.clone();
        self.green -= c.clone();
        self.blue -= c;
    }
}

impl<S, T> Mul<Rgb<S, T>> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: Mul,
{
    type Output = Rgb<S, <T as Mul>::Output>;

    fn mul(self, other: Rgb<S, T>) -> Self::Output {
        Rgb {
            red: self.red * other.red,
            green: self.green * other.green,
            blue: self.blue * other.blue,
            standard: PhantomData,
        }
    }
}

impl<S, T> Mul<T> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: Mul + Clone,
{
    type Output = Rgb<S, <T as Mul>::Output>;

    fn mul(self, c: T) -> Self::Output {
        Rgb {
            red: self.red * c.clone(),
            green: self.green * c.clone(),
            blue: self.blue * c,
            standard: PhantomData,
        }
    }
}

impl<S, T> MulAssign<Rgb<S, T>> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: MulAssign,
{
    fn mul_assign(&mut self, other: Rgb<S, T>) {
        self.red *= other.red;
        self.green *= other.green;
        self.blue *= other.blue;
    }
}

impl<S, T> MulAssign<T> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: MulAssign + Clone,
{
    fn mul_assign(&mut self, c: T) {
        self.red *= c.clone();
        self.green *= c.clone();
        self.blue *= c;
    }
}

impl<S, T> Div<Rgb<S, T>> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: Div,
{
    type Output = Rgb<S, <T as Div>::Output>;

    fn div(self, other: Rgb<S, T>) -> Self::Output {
        Rgb {
            red: self.red / other.red,
            green: self.green / other.green,
            blue: self.blue / other.blue,
            standard: PhantomData,
        }
    }
}

impl<S, T> Div<T> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: Div + Clone,
{
    type Output = Rgb<S, <T as Div>::Output>;

    fn div(self, c: T) -> Self::Output {
        Rgb {
            red: self.red / c.clone(),
            green: self.green / c.clone(),
            blue: self.blue / c,
            standard: PhantomData,
        }
    }
}

impl<S, T> DivAssign<Rgb<S, T>> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: DivAssign,
{
    fn div_assign(&mut self, other: Rgb<S, T>) {
        self.red /= other.red;
        self.green /= other.green;
        self.blue /= other.blue;
    }
}

impl<S, T> DivAssign<T> for Rgb<S, T>
where
    S: RgbStandard<T, TransferFn = LinearFn>,
    T: DivAssign + Clone,
{
    fn div_assign(&mut self, c: T) {
        self.red /= c.clone();
        self.green /= c.clone();
        self.blue /= c;
    }
}

impl<S, T> From<(T, T, T)> for Rgb<S, T> {
    fn from(components: (T, T, T)) -> Self {
        Self::from_components(components)
    }
}

impl<S, T> From<Rgb<S, T>> for (T, T, T) {
    fn from(color: Rgb<S, T>) -> (T, T, T) {
        color.into_components()
    }
}

impl<S, T, A> From<(T, T, T, A)> for Alpha<Rgb<S, T>, A> {
    fn from(components: (T, T, T, A)) -> Self {
        Self::from_components(components)
    }
}

impl<S, T, A> From<Alpha<Rgb<S, T>, A>> for (T, T, T, A) {
    fn from(color: Alpha<Rgb<S, T>, A>) -> (T, T, T, A) {
        color.into_components()
    }
}

impl<S, T> AbsDiffEq for Rgb<S, T>
where
    T: AbsDiffEq,
    T::Epsilon: Clone,
{
    type Epsilon = T::Epsilon;

    fn default_epsilon() -> Self::Epsilon {
        T::default_epsilon()
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.red.abs_diff_eq(&other.red, epsilon.clone())
            && self.green.abs_diff_eq(&other.green, epsilon.clone())
            && self.blue.abs_diff_eq(&other.blue, epsilon)
    }
}

impl<S, T> RelativeEq for Rgb<S, T>
where
    T: RelativeEq,
    T::Epsilon: Clone,
{
    fn default_max_relative() -> Self::Epsilon {
        T::default_max_relative()
    }

    #[rustfmt::skip]
    fn relative_eq(
        &self,
        other: &Self,
        epsilon: Self::Epsilon,
        max_relative: Self::Epsilon,
    ) -> bool {
        self.red.relative_eq(&other.red, epsilon.clone(), max_relative.clone()) &&
            self.green.relative_eq(&other.green, epsilon.clone(), max_relative.clone()) &&
            self.blue.relative_eq(&other.blue, epsilon, max_relative)
    }
}

impl<S, T> UlpsEq for Rgb<S, T>
where
    T: UlpsEq,
    T::Epsilon: Clone,
{
    fn default_max_ulps() -> u32 {
        T::default_max_ulps()
    }

    #[rustfmt::skip]
    fn ulps_eq(&self, other: &Self, epsilon: Self::Epsilon, max_ulps: u32) -> bool {
        self.red.ulps_eq(&other.red, epsilon.clone(), max_ulps) &&
            self.green.ulps_eq(&other.green, epsilon.clone(), max_ulps) &&
            self.blue.ulps_eq(&other.blue, epsilon, max_ulps)
    }
}

impl_array_casts!(Rgb<S, T>, [T; 3]);

impl<S, T> fmt::LowerHex for Rgb<S, T>
where
    T: fmt::LowerHex,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let size = f.width().unwrap_or(::core::mem::size_of::<T>() * 2);
        write!(
            f,
            "{:0width$x}{:0width$x}{:0width$x}",
            self.red,
            self.green,
            self.blue,
            width = size
        )
    }
}

impl<S, T> fmt::UpperHex for Rgb<S, T>
where
    T: fmt::UpperHex,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let size = f.width().unwrap_or(::core::mem::size_of::<T>() * 2);
        write!(
            f,
            "{:0width$X}{:0width$X}{:0width$X}",
            self.red,
            self.green,
            self.blue,
            width = size
        )
    }
}

/// Error type for parsing a string of hexadecimal characters to an `Rgb` color.
#[derive(Debug)]
pub enum FromHexError {
    /// An error occurred while parsing the string into a valid integer.
    ParseIntError(ParseIntError),
    /// The hex value was not in a valid 3 or 6 character format.
    HexFormatError(&'static str),
}

impl From<ParseIntError> for FromHexError {
    fn from(err: ParseIntError) -> FromHexError {
        FromHexError::ParseIntError(err)
    }
}

impl From<&'static str> for FromHexError {
    fn from(err: &'static str) -> FromHexError {
        FromHexError::HexFormatError(err)
    }
}
impl core::fmt::Display for FromHexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self {
            FromHexError::ParseIntError(e) => write!(f, "{}", e),
            FromHexError::HexFormatError(s) => write!(
                f,
                "{}, please use format '#fff', 'fff', '#ffffff' or 'ffffff'.",
                s
            ),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for FromHexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &*self {
            FromHexError::HexFormatError(_s) => None,
            FromHexError::ParseIntError(e) => Some(e),
        }
    }
}

impl<S> FromStr for Rgb<S, u8> {
    type Err = FromHexError;

    // Parses a color hex code of format '#ff00bb' or '#abc' into a
    // Rgb<S, u8> instance.
    fn from_str(hex: &str) -> Result<Self, Self::Err> {
        let hex_code = hex.strip_prefix('#').map_or(hex, |stripped| stripped);
        match hex_code.len() {
            3 => {
                let red = u8::from_str_radix(&hex_code[..1], 16)?;
                let green = u8::from_str_radix(&hex_code[1..2], 16)?;
                let blue = u8::from_str_radix(&hex_code[2..3], 16)?;
                let col: Rgb<S, u8> = Rgb::new(red * 17, green * 17, blue * 17);
                Ok(col)
            }
            6 => {
                let red = u8::from_str_radix(&hex_code[..2], 16)?;
                let green = u8::from_str_radix(&hex_code[2..4], 16)?;
                let blue = u8::from_str_radix(&hex_code[4..6], 16)?;
                let col: Rgb<S, u8> = Rgb::new(red, green, blue);
                Ok(col)
            }
            _ => Err("invalid hex code format".into()),
        }
    }
}

impl<S, T, P, O> From<Rgb<S, T>> for Packed<O, P>
where
    O: ComponentOrder<Rgba<S, T>, P>,
    Rgba<S, T>: From<Rgb<S, T>>,
{
    #[inline]
    fn from(color: Rgb<S, T>) -> Self {
        Self::from(Rgba::from(color))
    }
}

impl<S, T, O, P> From<Rgba<S, T>> for Packed<O, P>
where
    O: ComponentOrder<Rgba<S, T>, P>,
{
    #[inline]
    fn from(color: Rgba<S, T>) -> Self {
        Packed::pack(color)
    }
}

impl<S, O, P> From<Packed<O, P>> for Rgb<S, u8>
where
    O: ComponentOrder<Rgba<S, u8>, P>,
{
    #[inline]
    fn from(packed: Packed<O, P>) -> Self {
        Rgba::from(packed).color
    }
}

impl<S, T, O, P> From<Packed<O, P>> for Rgba<S, T>
where
    O: ComponentOrder<Rgba<S, T>, P>,
{
    #[inline]
    fn from(packed: Packed<O, P>) -> Self {
        packed.unpack()
    }
}

impl<S> From<u32> for Rgb<S, u8> {
    #[inline]
    fn from(color: u32) -> Self {
        Self::from_u32::<super::channels::Argb>(color)
    }
}

impl<S> From<u32> for Rgba<S, u8> {
    #[inline]
    fn from(color: u32) -> Self {
        Self::from_u32::<super::channels::Rgba>(color)
    }
}

impl<S> From<Rgb<S, u8>> for u32 {
    #[inline]
    fn from(color: Rgb<S, u8>) -> Self {
        Rgb::into_u32::<super::channels::Argb>(color)
    }
}

impl<S> From<Rgba<S, u8>> for u32 {
    #[inline]
    fn from(color: Rgba<S, u8>) -> Self {
        Rgba::into_u32::<super::channels::Rgba>(color)
    }
}

impl<S, T> RelativeContrast for Rgb<S, T>
where
    T: FloatComponent,
    S: RgbStandard<T>,
{
    type Scalar = T;

    #[inline]
    fn get_contrast_ratio(self, other: Self) -> T {
        use crate::FromColor;

        let xyz1 = Xyz::from_color(self);
        let xyz2 = Xyz::from_color(other);

        contrast_ratio(xyz1.y, xyz2.y)
    }
}

#[cfg(feature = "random")]
impl<S, T> Distribution<Rgb<S, T>> for Standard
where
    Standard: Distribution<T>,
{
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Rgb<S, T> {
        Rgb {
            red: rng.gen(),
            green: rng.gen(),
            blue: rng.gen(),
            standard: PhantomData,
        }
    }
}

#[cfg(feature = "random")]
pub struct UniformRgb<S, T>
where
    T: SampleUniform,
{
    red: Uniform<T>,
    green: Uniform<T>,
    blue: Uniform<T>,
    standard: PhantomData<S>,
}

#[cfg(feature = "random")]
impl<S, T> SampleUniform for Rgb<S, T>
where
    T: SampleUniform + Clone,
{
    type Sampler = UniformRgb<S, T>;
}

#[cfg(feature = "random")]
impl<S, T> UniformSampler for UniformRgb<S, T>
where
    T: SampleUniform + Clone,
{
    type X = Rgb<S, T>;

    fn new<B1, B2>(low_b: B1, high_b: B2) -> Self
    where
        B1: SampleBorrow<Self::X> + Sized,
        B2: SampleBorrow<Self::X> + Sized,
    {
        let low = low_b.borrow();
        let high = high_b.borrow();

        UniformRgb {
            red: Uniform::new::<_, T>(low.red.clone(), high.red.clone()),
            green: Uniform::new::<_, T>(low.green.clone(), high.green.clone()),
            blue: Uniform::new::<_, T>(low.blue.clone(), high.blue.clone()),
            standard: PhantomData,
        }
    }

    fn new_inclusive<B1, B2>(low_b: B1, high_b: B2) -> Self
    where
        B1: SampleBorrow<Self::X> + Sized,
        B2: SampleBorrow<Self::X> + Sized,
    {
        let low = low_b.borrow();
        let high = high_b.borrow();

        UniformRgb {
            red: Uniform::new_inclusive::<_, T>(low.red.clone(), high.red.clone()),
            green: Uniform::new_inclusive::<_, T>(low.green.clone(), high.green.clone()),
            blue: Uniform::new_inclusive::<_, T>(low.blue.clone(), high.blue.clone()),
            standard: PhantomData,
        }
    }

    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Rgb<S, T> {
        Rgb {
            red: self.red.sample(rng),
            green: self.green.sample(rng),
            blue: self.blue.sample(rng),
            standard: PhantomData,
        }
    }
}

#[cfg(feature = "bytemuck")]
unsafe impl<S, T> bytemuck::Zeroable for Rgb<S, T> where T: bytemuck::Zeroable {}

#[cfg(feature = "bytemuck")]
unsafe impl<S: 'static, T> bytemuck::Pod for Rgb<S, T> where T: bytemuck::Pod {}

#[cfg(test)]
mod test {
    use core::str::FromStr;

    use super::{Rgb, Rgba};
    use crate::encoding::Srgb;
    use crate::rgb::channels;

    #[test]
    fn ranges() {
        assert_ranges! {
            Rgb<Srgb, f64>;
            clamped {
                red: 0.0 => 1.0,
                green: 0.0 => 1.0,
                blue: 0.0 => 1.0
            }
            clamped_min {}
            unclamped {}
        }
    }

    raw_pixel_conversion_tests!(Rgb<Srgb>: red, green, blue);
    raw_pixel_conversion_fail_tests!(Rgb<Srgb>: red, green, blue);

    #[test]
    fn lower_hex() {
        assert_eq!(
            format!("{:x}", Rgb::<Srgb, u8>::new(171, 193, 35)),
            "abc123"
        );
    }

    #[test]
    fn lower_hex_small_numbers() {
        assert_eq!(format!("{:x}", Rgb::<Srgb, u8>::new(1, 2, 3)), "010203");
        assert_eq!(
            format!("{:x}", Rgb::<Srgb, u16>::new(1, 2, 3)),
            "000100020003"
        );
        assert_eq!(
            format!("{:x}", Rgb::<Srgb, u32>::new(1, 2, 3)),
            "000000010000000200000003"
        );
        assert_eq!(
            format!("{:x}", Rgb::<Srgb, u64>::new(1, 2, 3)),
            "000000000000000100000000000000020000000000000003"
        );
    }

    #[test]
    fn lower_hex_custom_width() {
        assert_eq!(
            format!("{:03x}", Rgb::<Srgb, u8>::new(1, 2, 3)),
            "001002003"
        );
        assert_eq!(
            format!("{:03x}", Rgb::<Srgb, u16>::new(1, 2, 3)),
            "001002003"
        );
        assert_eq!(
            format!("{:03x}", Rgb::<Srgb, u32>::new(1, 2, 3)),
            "001002003"
        );
        assert_eq!(
            format!("{:03x}", Rgb::<Srgb, u64>::new(1, 2, 3)),
            "001002003"
        );
    }

    #[test]
    fn upper_hex() {
        assert_eq!(
            format!("{:X}", Rgb::<Srgb, u8>::new(171, 193, 35)),
            "ABC123"
        );
    }

    #[test]
    fn upper_hex_small_numbers() {
        assert_eq!(format!("{:X}", Rgb::<Srgb, u8>::new(1, 2, 3)), "010203");
        assert_eq!(
            format!("{:X}", Rgb::<Srgb, u16>::new(1, 2, 3)),
            "000100020003"
        );
        assert_eq!(
            format!("{:X}", Rgb::<Srgb, u32>::new(1, 2, 3)),
            "000000010000000200000003"
        );
        assert_eq!(
            format!("{:X}", Rgb::<Srgb, u64>::new(1, 2, 3)),
            "000000000000000100000000000000020000000000000003"
        );
    }

    #[test]
    fn upper_hex_custom_width() {
        assert_eq!(
            format!("{:03X}", Rgb::<Srgb, u8>::new(1, 2, 3)),
            "001002003"
        );
        assert_eq!(
            format!("{:03X}", Rgb::<Srgb, u16>::new(1, 2, 3)),
            "001002003"
        );
        assert_eq!(
            format!("{:03X}", Rgb::<Srgb, u32>::new(1, 2, 3)),
            "001002003"
        );
        assert_eq!(
            format!("{:03X}", Rgb::<Srgb, u64>::new(1, 2, 3)),
            "001002003"
        );
    }

    #[test]
    fn rgb_hex_into_from() {
        let c1 = Rgb::<Srgb, u8>::from_u32::<channels::Argb>(0x1100_7FFF);
        let c2 = Rgb::<Srgb, u8>::new(0u8, 127, 255);
        assert_eq!(c1, c2);
        assert_eq!(Rgb::<Srgb, u8>::into_u32::<channels::Argb>(c1), 0xFF00_7FFF);

        let c1 = Rgba::<Srgb, u8>::from_u32::<channels::Rgba>(0x007F_FF80);
        let c2 = Rgba::<Srgb, u8>::new(0u8, 127, 255, 128);
        assert_eq!(c1, c2);
        assert_eq!(
            Rgba::<Srgb, u8>::into_u32::<channels::Rgba>(c1),
            0x007F_FF80
        );

        assert_eq!(
            Rgb::<Srgb, u8>::from(0x7FFF_FF80),
            Rgb::from((255u8, 255, 128))
        );
        assert_eq!(
            Rgba::<Srgb, u8>::from(0x7FFF_FF80),
            Rgba::from((127u8, 255, 255, 128))
        );
    }

    #[cfg(feature = "serializing")]
    #[test]
    fn serialize() {
        let serialized = ::serde_json::to_string(&Rgb::<Srgb>::new(0.3, 0.8, 0.1)).unwrap();

        assert_eq!(serialized, r#"{"red":0.3,"green":0.8,"blue":0.1}"#);
    }

    #[cfg(feature = "serializing")]
    #[test]
    fn deserialize() {
        let deserialized: Rgb<Srgb> =
            ::serde_json::from_str(r#"{"red":0.3,"green":0.8,"blue":0.1}"#).unwrap();

        assert_eq!(deserialized, Rgb::<Srgb>::new(0.3, 0.8, 0.1));
    }

    #[test]
    fn from_str() {
        let c = Rgb::<Srgb, u8>::from_str("#ffffff");
        assert!(c.is_ok());
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(255, 255, 255));
        let c = Rgb::<Srgb, u8>::from_str("#gggggg");
        assert!(c.is_err());
        let c = Rgb::<Srgb, u8>::from_str("#fff");
        assert!(c.is_ok());
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(255, 255, 255));
        let c = Rgb::<Srgb, u8>::from_str("#000000");
        assert!(c.is_ok());
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(0, 0, 0));
        let c = Rgb::<Srgb, u8>::from_str("");
        assert!(c.is_err());
        let c = Rgb::<Srgb, u8>::from_str("#123456");
        assert!(c.is_ok());
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(18, 52, 86));
        let c = Rgb::<Srgb, u8>::from_str("#iii");
        assert!(c.is_err());
        assert_eq!(
            format!("{}", c.err().unwrap()),
            "invalid digit found in string"
        );
        let c = Rgb::<Srgb, u8>::from_str("#08f");
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(0, 136, 255));
        let c = Rgb::<Srgb, u8>::from_str("08f");
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(0, 136, 255));
        let c = Rgb::<Srgb, u8>::from_str("ffffff");
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(255, 255, 255));
        let c = Rgb::<Srgb, u8>::from_str("#12");
        assert!(c.is_err());
        assert_eq!(
            format!("{}", c.err().unwrap()),
            "invalid hex code format, \
             please use format \'#fff\', \'fff\', \'#ffffff\' or \'ffffff\'."
        );
        let c = Rgb::<Srgb, u8>::from_str("da0bce");
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(218, 11, 206));
        let c = Rgb::<Srgb, u8>::from_str("f034e6");
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(240, 52, 230));
        let c = Rgb::<Srgb, u8>::from_str("abc");
        assert_eq!(c.unwrap(), Rgb::<Srgb, u8>::new(170, 187, 204));
    }

    #[test]
    fn check_min_max_components() {
        assert_relative_eq!(Rgb::<Srgb, f32>::min_red(), 0.0);
        assert_relative_eq!(Rgb::<Srgb, f32>::min_green(), 0.0);
        assert_relative_eq!(Rgb::<Srgb, f32>::min_blue(), 0.0);
        assert_relative_eq!(Rgb::<Srgb, f32>::max_red(), 1.0);
        assert_relative_eq!(Rgb::<Srgb, f32>::max_green(), 1.0);
        assert_relative_eq!(Rgb::<Srgb, f32>::max_blue(), 1.0);
    }

    #[cfg(feature = "random")]
    test_uniform_distribution! {
        Rgb<Srgb, f32> {
            red: (0.0, 1.0),
            green: (0.0, 1.0),
            blue: (0.0, 1.0)
        },
        min: Rgb::new(0.0f32, 0.0, 0.0),
        max: Rgb::new(1.0, 1.0, 1.0)
    }
}
